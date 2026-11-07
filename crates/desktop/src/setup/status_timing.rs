//! The status check's timing (detection among it: a CLI run in the lawyer's home), for host.log:
//! one line when desktop asks `Agents` for an agent's status check, and one when that agent's
//! next sign-in state reaches the windows, with `took_ms` between. The check answers only
//! through the setup feed, never to a caller, so this pairing is how the log measures it. A
//! check that could not tell sends no state and logs no answer. No path, email or code: the
//! agent and the state's kind only. The first [`LINES`] lines of a run.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use clauseo_protocol::AgentId;

use crate::bridge::lock;
use crate::chooser::trace::{at_ms, since};

/// The target: host.log keeps info only from `clauseo_*::lifecycle` targets.
pub(crate) const TARGET: &str = "clauseo_desktop::setup::lifecycle";

/// How many lines a run logs.
const LINES: u32 = 50;

/// The lines logged so far.
static LOGGED: AtomicU32 = AtomicU32::new(0);

/// When each agent's status check was asked for, until its state comes.
static ASKED: Mutex<BTreeMap<AgentId, Instant>> = Mutex::new(BTreeMap::new());

/// Whether one more line fits under the cap.
fn room() -> bool {
    LOGGED.fetch_add(1, Ordering::Relaxed) < LINES
}

/// Desktop asked for `agent`'s status check (`why`: `installed` or `auth_required`).
pub(crate) fn asked(agent: &AgentId, why: &'static str) {
    lock(&ASKED).insert(agent.clone(), crate::open::now());
    if room() {
        tracing::info!(target: TARGET, at_ms = at_ms(), agent = agent.as_str(), why,
            "sign-in status check asked");
    }
}

/// `agent`'s sign-in state of kind `state` is going to the windows: the answer to a check
/// asked for, if one waits.
pub(crate) fn answered(agent: &AgentId, state: &str) {
    let Some(began) = lock(&ASKED).remove(agent) else {
        return;
    };
    if room() {
        tracing::info!(target: TARGET, at_ms = at_ms(), took_ms = since(began),
            agent = agent.as_str(), state, "sign-in status check answered");
    }
}

#[cfg(test)]
mod tests;
