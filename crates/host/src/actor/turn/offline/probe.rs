//! The host's own word before an offline watch ends a turn: the webview's `offline` can be
//! wrong or stuck, an agent's note can come before a retry that gets through, and a quiet agent
//! may be thinking. So a thread of its own asks whether the one service the agent's login uses
//! answers, within [`PROBE_CAP`] for the whole try, and posts back a [`Reach`]
//! (`probe/route.rs`): behind a proxy (the firm's settings, the host's variables or the OS's, as
//! the egress proxy routes, decision 0039) a `CONNECT` whose status counts, then a TLS
//! client hello whose first answer record counts; straight to the host the same client hello. No
//! certificate is checked and nothing but the hello is sent. The log gets fixed fields only.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::{AgentKind, ThreadId, TurnId};
use url::Url;

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::TurnInput;
use crate::log_file::{Cause, LIFECYCLE};

mod route;

#[cfg(test)]
mod tests;

/// The most the whole try may take: the lookup, the TCP connection, the `CONNECT` head and the
/// first TLS record.
pub(super) const PROBE_CAP: Duration = Duration::from_secs(8);

/// The thread's name.
const THREAD_NAME: &str = "clauseo-offline-probe";

/// The service Codex uses with its own sign-in (`chatgpt.com`).
const CHATGPT: &str = "https://chatgpt.com";

/// The service Codex uses with an API key.
const OPENAI_API: &str = "https://api.openai.com";

/// Claude's service.
const ANTHROPIC_API: &str = "https://api.anthropic.com";

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
    /// The agent has no known service.
    NoService,
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
        }
    }
}

/// Whether the probe went through a proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Via {
    Proxy,
    Direct,
}

impl Via {
    /// Its word in the log.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Proxy => "proxy",
            Self::Direct => "direct",
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
            via: Via::Direct,
            status: 0,
            ms: 0,
        }
    }

    /// Whether the service (or the firm's proxy, for it) answered.
    pub(crate) const fn reached(self) -> bool {
        matches!(
            self.class,
            ReachClass::Answered | ReachClass::ProxyAuth | ReachClass::Plain
        )
    }
}

/// Probes the agent's service (the URL, `None` for an agent of no known kind). Blocking; it
/// runs on the probe's thread. Tests set fixed answers.
#[derive(Clone)]
pub(crate) struct ReachProbe(Arc<Probe>);

/// The probe's call.
type Probe = dyn Fn(Option<&Url>) -> Reach + Send + Sync;

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
        let probe = Self(Arc::new(move |url| {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            (self.0)(url)
        }));
        (probe, count)
    }
}

impl Default for ReachProbe {
    /// The real probe; in unit tests one that never gets through, so none touches the network.
    fn default() -> Self {
        if cfg!(test) {
            Self(Arc::new(|_| Reach::of(ReachClass::Refused)))
        } else {
            Self(Arc::new(|url| {
                url.map_or(Reach::of(ReachClass::NoService), |url| {
                    route::probe(url, PROBE_CAP)
                })
            }))
        }
    }
}

impl fmt::Debug for ReachProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReachProbe")
    }
}

/// The one service `thread`'s agent talks to: the base URL its variables set (the agent's
/// settings, then the host's own); else the host its login uses: Codex's `api.openai.com` when
/// its launch passed a key (`CODEX_API_KEY`, `agents/launch.rs`), its `chatgpt.com` otherwise,
/// Claude's `api.anthropic.com`. A firm that blocks `chatgpt.com` but allows `api.openai.com`
/// must not read as reached for a `chatgpt.com` login. `None` for an agent of no known kind.
pub(super) fn service(actor: &mut Actor, thread: ThreadId) -> Option<Url> {
    let agent = actor.threads.get(&thread)?.row.agent.clone();
    let kind = AgentKind::of(&agent);
    // The unit tests' `fake-agent` has no kind; a test names the one it plays.
    #[cfg(test)]
    let kind = kind.or(actor.turn_path.kind_for_tests);
    let kind = kind?;
    let key = super::super::updates::agent(actor, thread)
        .and_then(|agent| agent.codex.as_ref())
        .is_some_and(|codex| codex.api_key);
    let (var, default) = match kind {
        AgentKind::Codex if key => ("OPENAI_BASE_URL", OPENAI_API),
        AgentKind::Codex => ("OPENAI_BASE_URL", CHATGPT),
        AgentKind::Claude => ("ANTHROPIC_BASE_URL", ANTHROPIC_API),
    };
    let set = actor
        .effective
        .agent(agent.as_str())
        .and_then(|launch| launch.env.get(var).cloned())
        .or_else(|| std::env::var(var).ok());
    set.and_then(|set| Url::parse(&set).ok())
        .or_else(|| Url::parse(default).ok())
}

/// Starts the probe for `thread`'s `turn`: its answer comes back as
/// [`TurnInput::OfflineProbe`]. A thread that does not start answers "not reached" at once.
pub(super) fn start(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let url = service(actor, thread);
    let probe = actor.turn_path.probe.clone();
    let post = actor.post.clone();
    let spawned = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let reach = (probe.0)(url.as_ref());
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
