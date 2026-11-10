//! The host's own word on a watch only the window armed (R2-1): the webview's `offline` can be
//! wrong or stuck, so before such a watch ends a turn, a thread of its own resolves the agent's
//! service host and opens one TCP connection to it, within [`PROBE_CAP`], and posts back whether
//! it got through. Behind a proxy (the firm's settings, the host's variables or the OS's, as the
//! egress proxy routes, decision 0039) it tries the proxy instead. Nothing is sent, and the log
//! gets fixed fields only.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::{AgentKind, ThreadId, TurnId};
use clauseo_sandbox::proxy::upstream::reachable;
use clauseo_sandbox::proxy::{Route, Upstream, upstream_for};
use url::Url;

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::TurnInput;
use crate::log_file::Cause;

/// The most the connection may take, its lookup included.
const PROBE_CAP: Duration = Duration::from_secs(3);

/// The thread's name.
const THREAD_NAME: &str = "clauseo-offline-probe";

/// Whether the agent's service (the URL, `None` for an agent of no known kind) takes a
/// connection. Blocking; it runs on the probe's thread. Tests set a fixed answer.
#[derive(Clone)]
pub(crate) struct ReachProbe(Arc<Probe>);

/// The probe's call.
type Probe = dyn Fn(Option<&Url>) -> bool + Send + Sync;

impl ReachProbe {
    /// A probe that answers `reached` without trying anything.
    #[cfg(test)]
    pub(crate) fn fixed(reached: bool) -> Self {
        Self(Arc::new(move |_| reached))
    }
}

impl Default for ReachProbe {
    fn default() -> Self {
        Self(Arc::new(connects))
    }
}

impl fmt::Debug for ReachProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReachProbe")
    }
}

/// The real probe: the route the egress proxy would take to `url`, then one connection to the
/// proxy or the host. An agent of no known kind has no host to try, so the window's word stands.
fn connects(url: Option<&Url>) -> bool {
    let Some(url) = url else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let port = url.port_or_known_default().unwrap_or(443);
    match upstream_for(&Upstream::System, url) {
        Ok(Route::Http { host, port }) => reachable(&host, port, PROBE_CAP),
        Ok(Route::Direct) | Err(_) => reachable(host, port, PROBE_CAP),
    }
}

/// The service `thread`'s agent talks to: the base URL its variables set (the agent's settings,
/// then the host's own), else Codex's `chatgpt.com` or Claude's `api.anthropic.com`.
fn service(actor: &Actor, thread: ThreadId) -> Option<Url> {
    let agent = &actor.threads.get(&thread)?.row.agent;
    let (var, default) = match AgentKind::of(agent)? {
        AgentKind::Codex => ("OPENAI_BASE_URL", "https://chatgpt.com"),
        AgentKind::Claude => ("ANTHROPIC_BASE_URL", "https://api.anthropic.com"),
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
            let reached = (probe.0)(url.as_ref());
            let input = TurnInput::OfflineProbe {
                thread,
                turn,
                reached,
            };
            let _gone = post.try_send(Input::Turn(input));
        });
    if let Err(error) = spawned {
        tracing::warn!(%thread, error = %Cause(&error), "the offline probe did not start");
        super::on_probed(actor, thread, turn, false);
    }
}
