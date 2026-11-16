//! The route for one connection, and what a failed OS lookup means for it (decision record 0039,
//! "A failed lookup"). A PAC file that could not be fetched (usually a laptop off the firm's
//! network): the host's proxy from the last 10 minutes while it still takes a connection, else
//! direct, as browsers treat a PAC they cannot fetch. Any other failure: the host's last good
//! route from the last 10 minutes; else a refusal when the settings show the firm meant a proxy;
//! else direct.

#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::Ctx;
use crate::proxy::head::within;
use crate::proxy::target::{Kind, Target};
use crate::proxy::upstream::{Answer, Decided, FailKind, Failure, Route, Scheme};
use crate::proxy::{Exchange, Verdict};
use crate::{Denial, DenialSink};

/// How long a route a real lookup gave is reused over failed lookups: desktop's backstop for a
/// kept proxy (S17), so the two agree.
const REUSE_FOR: Duration = Duration::from_mins(10);

/// The longest the probe of an earlier proxy may take, over a PAC that could not be fetched.
pub(super) const PROBE_CAP: Duration = Duration::from_secs(1);

/// The most hosts remembered per proxy. At the cap a new host first drops the expired, then the
/// oldest, so random names under a firm wildcard cannot grow it.
pub(super) const REMEMBER_MOST: usize = 256;

/// A scheme, host and port, as the lookup is asked.
pub(super) type Key = (Scheme, String, u16);

/// Per key, the last route a real OS lookup gave and when, on the lookup's clock. Only the proxy
/// thread touches it, and never across an await.
#[derive(Default)]
pub(super) struct Remembered(RefCell<HashMap<Key, (Route, Duration)>>);

/// What a connection does with the lookup's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Chosen {
    /// Takes this route.
    Go(Route),
    /// Refused: the firm meant a proxy and none is known.
    Refuse,
}

/// What a connection does next with the lookup's answer.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Plan {
    /// Chosen, with no probe.
    Chosen(Chosen),
    /// Probes the host's earlier proxy, at this host and port, for [`Remembered::probed`].
    Probe { host: String, port: u16 },
}

impl Remembered {
    /// Keeps an OS route, or plans over a failure. A PAC that could not be fetched probes the
    /// host's earlier proxy, if any, and else goes direct, even when the firm meant a proxy: the
    /// proxy it meant cannot be reached. Any other failure reuses the host's earlier route; with
    /// none, it refuses when the firm meant a proxy, else goes direct.
    pub(super) fn plan(&self, key: &Key, decided: Decided) -> Plan {
        let Decided { answer, got, now } = decided;
        let (failure, kind) = match answer {
            Answer::Route(route) => {
                if let Some(got) = got {
                    self.keep(key.clone(), route.clone(), got, now);
                }
                return Plan::Chosen(Chosen::Go(route));
            }
            Answer::Failed(failure, kind) => (failure, kind),
        };
        let last = self
            .0
            .borrow()
            .get(key)
            .filter(|(_, got)| now.saturating_sub(*got) < REUSE_FOR)
            .map(|(route, _)| route.clone());
        if kind == FailKind::PacUnreachable {
            return match last {
                Some(Route::Http { host, port }) => Plan::Probe { host, port },
                Some(Route::Direct) | None => Plan::Chosen(Chosen::Go(Route::Direct)),
            };
        }
        Plan::Chosen(match (last, failure) {
            (Some(route), _) => Chosen::Go(route),
            (None, Failure::MeantProxy) => Chosen::Refuse,
            (None, Failure::Unmeant) => Chosen::Go(Route::Direct),
        })
    }

    /// After a [`Plan::Probe`] of `route`: kept when it took a connection; else forgotten, so
    /// no later failure reuses it, and direct.
    pub(super) fn probed(&self, key: &Key, route: Route, reached: bool) -> Chosen {
        if reached {
            return Chosen::Go(route);
        }
        let mut routes = self.0.borrow_mut();
        if routes.get(key).is_some_and(|(kept, _)| *kept == route) {
            routes.remove(key);
        }
        Chosen::Go(Route::Direct)
    }

    fn keep(&self, key: Key, route: Route, got: Duration, now: Duration) {
        let mut routes = self.0.borrow_mut();
        if routes.len() >= REMEMBER_MOST && !routes.contains_key(&key) {
            routes.retain(|_, (_, at)| now.saturating_sub(*at) < REUSE_FOR);
            while routes.len() >= REMEMBER_MOST {
                let oldest = routes
                    .iter()
                    .min_by_key(|(_, (_, at))| *at)
                    .map(|(key, _)| key.clone());
                let Some(oldest) = oldest else { break };
                routes.remove(&oldest);
            }
        }
        routes.insert(key, (route, got));
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.0.borrow().len()
    }
}

/// The route for `target` on `ctx`, or `None` when refused (the denial sent, the verdict set). A
/// lookup slower than the connect timeout counts as failed, the settings read for whether a
/// proxy was meant. A probe runs off the proxy thread, for at most [`PROBE_CAP`].
pub(super) async fn route(ctx: &Ctx, target: &Target, exchange: &mut Exchange) -> Option<Route> {
    let host = target.name.text();
    let port = target.port;
    let scheme = if matches!(target.kind, Kind::Connect) {
        Scheme::Https
    } else {
        Scheme::Http
    };
    let lookup = Arc::clone(&ctx.lookup);
    let asked = host.clone();
    let decide = blocking::unblock(move || lookup.decide(scheme, &asked, port));
    let decided = if let Some(decided) = within(ctx.limits.connect, decide).await {
        decided
    } else {
        tracing::warn!("egress: upstream lookup too slow; taken as failed");
        let lookup = Arc::clone(&ctx.lookup);
        let intent = blocking::unblock(move || lookup.intent());
        // A settings read that does not finish counts as a proxy meant: refuse, not direct.
        let failure = within(ctx.limits.connect, intent)
            .await
            .unwrap_or(Failure::MeantProxy);
        Decided {
            answer: Answer::Failed(failure, FailKind::Other),
            got: None,
            now: ctx.lookup.now(),
        }
    };
    let key = (scheme, host, port);
    let chosen = match ctx.remembered.plan(&key, decided) {
        Plan::Chosen(chosen) => chosen,
        Plan::Probe { host, port } => {
            let lookup = Arc::clone(&ctx.lookup);
            let asked = host.clone();
            let probe = blocking::unblock(move || lookup.reachable(&asked, port, PROBE_CAP));
            let reached = probe.await;
            if !reached {
                tracing::info!("egress: the PAC was not fetched and the earlier proxy is silent");
            }
            ctx.remembered
                .probed(&key, Route::Http { host, port }, reached)
        }
    };
    match chosen {
        Chosen::Go(route) => Some(route),
        Chosen::Refuse => {
            exchange.verdict = Verdict::Failed;
            let (host, port) = (&exchange.host, exchange.port);
            tracing::info!(%host, port, "egress: refused; the firm's proxy lookup failed");
            ctx.sink.denied(Denial::UpstreamUnavailable {
                host: exchange.host.clone(),
                port: exchange.port,
            });
            None
        }
    }
}
