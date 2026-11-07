//! The route for one connection, and what a failed OS lookup means for it (decision record 0039,
//! "A failed lookup"): the host's last good route from the last 10 minutes; else a refusal when
//! the settings show the firm meant a proxy; else direct.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::Ctx;
use crate::proxy::head::within;
use crate::proxy::target::{Kind, Target};
use crate::proxy::upstream::{Answer, Decided, Failure, Route, Scheme};
use crate::proxy::{Exchange, Verdict};
use crate::{Denial, DenialSink};

/// How long a route a real lookup gave is reused over failed lookups: desktop's backstop for a
/// kept proxy (S17), so the two agree.
const REUSE_FOR: Duration = Duration::from_mins(10);

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
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Chosen {
    /// Takes this route.
    Go(Route),
    /// Refused: the firm meant a proxy and none is known.
    Refuse,
}

impl Remembered {
    /// Keeps an OS route, or chooses over a failure.
    pub(super) fn choose(&self, key: Key, decided: Decided) -> Chosen {
        let Decided { answer, got, now } = decided;
        match answer {
            Answer::Route(route) => {
                if let Some(got) = got {
                    self.keep(key, route.clone(), got, now);
                }
                Chosen::Go(route)
            }
            Answer::Failed(failure) => {
                let last = self
                    .0
                    .borrow()
                    .get(&key)
                    .filter(|(_, got)| now.saturating_sub(*got) < REUSE_FOR)
                    .map(|(route, _)| route.clone());
                match (last, failure) {
                    (Some(route), _) => Chosen::Go(route),
                    (None, Failure::MeantProxy) => Chosen::Refuse,
                    (None, Failure::Unmeant) => Chosen::Go(Route::Direct),
                }
            }
        }
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
/// proxy was meant.
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
            answer: Answer::Failed(failure),
            got: None,
            now: ctx.lookup.now(),
        }
    };
    match ctx.remembered.choose((scheme, host, port), decided) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn key(host: &str) -> Key {
        (Scheme::Https, host.to_owned(), 443)
    }

    fn proxy() -> Route {
        Route::Http {
            host: "proxy.firm.test".to_owned(),
            port: 8080,
        }
    }

    fn got(at: Duration) -> Decided {
        Decided {
            answer: Answer::Route(proxy()),
            got: Some(at),
            now: at,
        }
    }

    fn failed(now: Duration) -> Decided {
        Decided {
            answer: Answer::Failed(Failure::MeantProxy),
            got: None,
            now,
        }
    }

    /// Past the cap the oldest host goes first, and the newest is still reused.
    #[test]
    fn the_cap_holds_and_drops_the_oldest() {
        let remembered = Remembered::default();
        let extra = 10;
        for n in 0..REMEMBER_MOST + extra {
            let at = Duration::from_secs(u64::try_from(n).unwrap());
            remembered.choose(key(&format!("h{n}.test")), got(at));
        }
        assert_eq!(remembered.len(), REMEMBER_MOST);
        let now = Duration::from_secs(u64::try_from(REMEMBER_MOST + extra).unwrap());
        assert_eq!(
            remembered.choose(key("h0.test"), failed(now)),
            Chosen::Refuse
        );
        let newest = format!("h{}.test", REMEMBER_MOST + extra - 1);
        assert_eq!(
            remembered.choose(key(&newest), failed(now)),
            Chosen::Go(proxy())
        );
    }

    /// A route from the settings or the environment (no OS time) is not remembered.
    #[test]
    fn a_route_not_from_the_os_is_not_remembered() {
        let remembered = Remembered::default();
        let plain = Decided {
            answer: Answer::Route(proxy()),
            got: None,
            now: Duration::ZERO,
        };
        assert_eq!(remembered.choose(key("a.test"), plain), Chosen::Go(proxy()));
        assert_eq!(remembered.len(), 0);
    }
}
