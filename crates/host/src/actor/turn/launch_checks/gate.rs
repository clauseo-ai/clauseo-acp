//! Desktop's launch gate on the launch path (`HostConfig::with_launch_gate`, R17 M3). Phase one
//! asks it on the prep thread ([`ask`]), after the data-folder refusal and before anything is
//! made, so a refused launch has no run folder to take back. Phase two, on the actor, acts on
//! a refusal ([`on_gated`]). Desktop sets the refusal itself before the gate answers; the host
//! never writes the gate's code into the launch refusals, and only reads them again after its
//! one wait.

use clauseo_protocol::{AgentId, ErrorCode, ThreadId};

use super::super::super::Actor;
use super::super::agents;
use super::super::{host_stopping, state};
use super::Prepared;
use crate::agent::launch_gate::{self, LaunchGate};
use crate::agent::launch_refusals::{self, WAITED_OUT};
use crate::error::Error;

/// What a launch's phase one found: its preparation, or the gate's refusal code.
pub(crate) enum Found {
    /// Every check passed and the session's folders are made.
    Prepared(Box<Prepared>),
    /// The gate refused the launch with this code; nothing was made.
    Gated(ErrorCode),
}

/// The gate and the agent it asks about, for `thread`'s launch, when a gate is set.
pub(super) fn of(actor: &Actor, thread: ThreadId) -> Option<(LaunchGate, AgentId)> {
    let gate = actor.wiring.launch_gate.clone()?;
    let agent = actor.threads.get(&thread)?.row.agent.clone();
    Some((gate, agent))
}

/// Asks the gate, on the prep thread: its refusal code, or none (no gate, or `None`); a panic
/// is `agent.launch_failed`.
pub(super) fn ask(gate: Option<(LaunchGate, AgentId)>) -> Result<Option<ErrorCode>, Error> {
    match gate {
        Some((gate, agent)) => launch_gate::ask(Some(&gate), &agent),
        None => Ok(None),
    }
}

/// Phase two of a launch the gate refused with `code` (`id`, the launch's checks): an answer
/// for an earlier launch is dropped, it made nothing. A code that is no launch refusal fails
/// the waiting work closed with `agent.launch_failed`; another refusal fails it with its code.
/// A waited-out one sends the launch back through `sign_in::keys_wait` and phase one again
/// (`agents::start_after_gate`), unless this launch already waited once, which fails it with
/// the code.
pub(in super::super) fn on_gated(actor: &mut Actor, thread: ThreadId, id: u64, code: ErrorCode) {
    let preparing = state(actor, thread)
        .and_then(|state| state.preparing.take_if(|preparing| preparing.id == id));
    let Some(preparing) = preparing else {
        return;
    };
    let super::Preparing {
        waiting,
        gate_waited,
        ..
    } = *preparing;
    if host_stopping(actor) {
        agents::fail_all(actor, thread, waiting, &Error::Closed);
        return;
    }
    let shown = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.to_string())
        .unwrap_or_default();
    if !launch_refusals::taken(code) {
        tracing::warn!(
            agent = %shown,
            code = code.as_str(),
            "the launch gate answered a code that is not a launch refusal; the launch fails"
        );
        agents::fail_all(actor, thread, waiting, &launch_gate::failed());
        return;
    }
    if !WAITED_OUT.contains(&code) || gate_waited {
        tracing::info!(agent = %shown, code = code.as_str(), "the launch gate refused a launch");
        let refused = Error::refused(code, "Clauseo is not starting this agent for now");
        agents::fail_all(actor, thread, waiting, &refused);
        return;
    }
    tracing::info!(
        agent = %shown,
        code = code.as_str(),
        "the launch gate refused a launch; it waits for the refusal to lift"
    );
    agents::start_after_gate(actor, thread, waiting);
}
