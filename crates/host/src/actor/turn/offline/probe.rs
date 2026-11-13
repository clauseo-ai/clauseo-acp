//! The host's own word before an offline watch ends a turn: the webview's `offline` can be
//! wrong or stuck, an agent's note can come before a retry that gets through, and a quiet agent
//! may be thinking. So a thread of its own asks whether the service the agent's login uses
//! answers, within [`PROBE_CAP`] for the whole try, and posts back a [`Reach`]
//! (`probe/route.rs`), by the service and route the agent's launch recorded (`probe/service.rs`,
//! read from the launch's final environment, so no route is looked up here): behind the proxy
//! the launch handed the agent a `CONNECT` whose status counts, then a TLS client hello whose
//! first answer record counts; straight to the host the same client hello. No certificate is
//! checked and nothing but the hello is sent. A launch whose service or route cannot be known is
//! not probed: it counts as reached at once. The log gets fixed fields only.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::{ThreadId, TurnId};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::TurnInput;
use crate::log_file::{Cause, LIFECYCLE};

mod route;
mod service;

pub(crate) use service::{Service, Target};

#[cfg(test)]
mod tests;

/// The most the whole try may take: the name's lookup, the TCP connection, the `CONNECT` head
/// and the first TLS record (each service of a login of unknown kind at once, side by side).
pub(super) const PROBE_CAP: Duration = Duration::from_secs(8);

/// The thread's name.
const THREAD_NAME: &str = "clauseo-offline-probe";

/// How a probe went, in fixed words only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReachClass {
    /// A TLS handshake record came back: the service, or a firm's proxy that answers for it.
    Answered,
    /// The proxy asks for credentials: counted as reached, so a proxy login never reads as
    /// offline (its blocked upstream is left to the quiet backstop).
    ProxyAuth,
    /// A plain-HTTP service took the TCP connection.
    Plain,
    /// The connection was refused or the network unreachable.
    Refused,
    /// The name did not resolve.
    NoName,
    /// The lookup or the connection ran past the cap.
    ConnectTimeout,
    /// The proxy answered the `CONNECT` with a status other than 200 or 407.
    TunnelRefused,
    /// The proxy took the connection but sent no answer head in time, or garbage, or closed.
    TunnelSilent,
    /// No TLS record came back in time.
    TlsSilent,
    /// Something other than a TLS handshake record came back (an alert, HTTP text, a close).
    TlsOther,
    /// The agent has no known kind (a probe of nothing: a miss).
    NoService,
    /// Not probed: the agent's service cannot be known (a cloud provider's login, a base URL
    /// that names no one host). Counted as reached.
    UnknownService,
    /// Not probed: the launch hands the agent a proxy a `CONNECT` cannot follow (`https://`,
    /// `socks5://`). Counted as reached.
    UnknownRoute,
}

impl ReachClass {
    /// Its word in the log.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::ProxyAuth => "proxy_auth",
            Self::Plain => "plain",
            Self::Refused => "refused",
            Self::NoName => "no_name",
            Self::ConnectTimeout => "connect_timeout",
            Self::TunnelRefused => "tunnel_refused",
            Self::TunnelSilent => "tunnel_silent",
            Self::TlsSilent => "tls_silent",
            Self::TlsOther => "tls_other",
            Self::NoService => "no_service",
            Self::UnknownService => "unknown_service",
            Self::UnknownRoute => "unknown_route",
        }
    }
}

/// Whether the probe went through a proxy (`None`: it tried nothing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Via {
    Proxy,
    Direct,
    None,
}

impl Via {
    /// Its word in the log.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Proxy => "proxy",
            Self::Direct => "direct",
            Self::None => "none",
        }
    }
}

/// A probe's answer: how it went, the route, the `CONNECT` status (0 when none came) and how
/// long it took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reach {
    pub(crate) class: ReachClass,
    pub(crate) via: Via,
    pub(crate) status: u16,
    pub(crate) ms: u32,
}

impl Reach {
    /// A probe that tried nothing.
    pub(crate) const fn of(class: ReachClass) -> Self {
        Self {
            class,
            via: Via::None,
            status: 0,
            ms: 0,
        }
    }

    /// Whether the service (or the firm's proxy, for it) answered.
    pub(crate) const fn reached(self) -> bool {
        matches!(
            self.class,
            ReachClass::Answered
                | ReachClass::ProxyAuth
                | ReachClass::Plain
                | ReachClass::UnknownService
                | ReachClass::UnknownRoute
        )
    }
}

/// Probes the agent's services (as its launch recorded them, `None` for an agent of no known
/// kind). Blocking; it runs on the probe's thread. Tests set fixed answers.
#[derive(Clone)]
pub(crate) struct ReachProbe(Arc<Probe>);

/// The probe's call.
type Probe = dyn Fn(Option<&[Service]>) -> Reach + Send + Sync;

/// A fixed answer of a test probe.
#[cfg(test)]
fn answer(reached: bool) -> Reach {
    if reached {
        Reach::of(ReachClass::Answered)
    } else {
        Reach::of(ReachClass::Refused)
    }
}

#[cfg(test)]
impl ReachProbe {
    /// A probe that answers `reached` without trying anything.
    pub(crate) fn fixed(reached: bool) -> Self {
        Self(Arc::new(move |_| answer(reached)))
    }

    /// A probe that answers each of `answers` in turn, then the last one again.
    pub(crate) fn sequence(answers: Vec<bool>) -> Self {
        let left = std::sync::Mutex::new(std::collections::VecDeque::from(answers));
        Self(Arc::new(move |_| {
            let mut left = left
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let next = if left.len() > 1 {
                left.pop_front()
            } else {
                left.front().copied()
            };
            answer(next.unwrap_or(false))
        }))
    }

    /// This probe, counting its calls in the counter it hands back.
    pub(crate) fn counted(self) -> (Self, Arc<std::sync::atomic::AtomicUsize>) {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        let probe = Self(Arc::new(move |services| {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            (self.0)(services)
        }));
        (probe, count)
    }

    /// This probe, keeping each call's services in the list it hands back.
    pub(crate) fn recorded(self) -> (Self, Arc<std::sync::Mutex<Vec<Vec<Service>>>>) {
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        let probe = Self(Arc::new(move |services: Option<&[Service]>| {
            seen.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(services.map(<[Service]>::to_vec).unwrap_or_default());
            (self.0)(services)
        }));
        (probe, calls)
    }
}

impl ReachProbe {
    /// The real probe, the one a host outside the unit tests gets.
    pub(crate) fn real() -> Self {
        Self(Arc::new(|services| {
            services.map_or(Reach::of(ReachClass::NoService), |services| {
                route::probe_any(services, PROBE_CAP)
            })
        }))
    }
}

impl Default for ReachProbe {
    /// The real probe; in unit tests one that never gets through, so none touches the network.
    fn default() -> Self {
        if cfg!(test) {
            Self(Arc::new(|_| Reach::of(ReachClass::Refused)))
        } else {
            Self::real()
        }
    }
}

impl fmt::Debug for ReachProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReachProbe")
    }
}

/// What `thread`'s agent launch recorded for the probe: `None` for an agent of no known kind
/// (or none running).
pub(super) fn target(actor: &mut Actor, thread: ThreadId) -> Option<Target> {
    super::super::updates::agent(actor, thread).and_then(|agent| agent.probe.clone())
}

/// Starts the probe for `thread`'s `turn`: its answer comes back as
/// [`TurnInput::OfflineProbe`]. A launch whose service or route cannot be known is not probed:
/// its class (reached) is answered at once, as is "not reached" when the thread does not start.
pub(super) fn start(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let services = match target(actor, thread) {
        Some(Target::Unknown(class)) => {
            super::on_probed(actor, thread, turn, Reach::of(class));
            return;
        }
        Some(Target::Service(services)) => Some(services),
        None => None,
    };
    let probe = actor.turn_path.probe.clone();
    let post = actor.post.clone();
    let spawned = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let reach = (probe.0)(services.as_deref());
            let input = TurnInput::OfflineProbe {
                thread,
                turn,
                reach,
            };
            // The internal channel is unbounded: a send fails only once the actor is gone.
            let _gone = post.try_send(Input::Turn(input));
        });
    if let Err(error) = spawned {
        tracing::warn!(%thread, error = %Cause(&error), "the offline probe did not start");
        super::on_probed(actor, thread, turn, Reach::of(ReachClass::NoService));
    }
}

/// Logs a probe's answer for `thread`'s watch, armed `by`, at the lifecycle target (host.log
/// keeps it): fixed fields only, never a host, a proxy, a URL or bytes.
pub(super) fn log(thread: ThreadId, reach: Reach, noted: bool, by: &str, misses: u8) {
    tracing::info!(
        target: LIFECYCLE,
        %thread,
        reached = reach.reached(),
        noted,
        by,
        class = reach.class.as_str(),
        via = reach.via.as_str(),
        status = reach.status,
        ms = reach.ms,
        misses,
        "the host's probe of the agent's service"
    );
}
