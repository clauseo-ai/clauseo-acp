//! Desktop's launch gate on the launch path (`HostConfig::with_launch_gate`, R17 M3). Phase one
//! asks it on the prep thread ([`last`]), once everything else is made, so its answer is not
//! staled by the rest of the preparation; a refusal takes back what was made. Phase two, on the
//! actor, acts on a refusal ([`on_gated`]). Desktop sets the refusal itself before the gate
//! answers; the host never writes the gate's code into the launch refusals, and reads them again
//! before it fails a launch with the gate's code.

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
/// is `agent.launch_failed` in debug and test builds, and aborts the process in release.
fn ask(gate: Option<(LaunchGate, AgentId)>) -> Result<Option<ErrorCode>, Error> {
    match gate {
        Some((gate, agent)) => launch_gate::ask(Some(&gate), &agent),
        None => Ok(None),
    }
}

/// The gate asked last, on the prep thread, over the launch's preparation (`prepared`): that
/// preparation when the gate lets it go on, else the gate's refusal or failure, with what the
/// preparation made taken back through `undo` first.
pub(super) fn last(
    gate: Option<(LaunchGate, AgentId)>,
    prepared: Found,
    undo: fn(Found),
) -> Result<Found, Error> {
    match ask(gate) {
        Ok(None) => Ok(prepared),
        Ok(Some(code)) => {
            undo(prepared);
            Ok(Found::Gated(code))
        }
        Err(error) => {
            undo(prepared);
            Err(error)
        }
    }
}

/// Phase two of a launch the gate refused with `code` (`id`, the launch's checks): an answer
/// for an earlier launch is dropped, its preparation already taken back. A code that is no
/// launch refusal fails the waiting work closed with `agent.launch_failed`. A waited-out one
/// sends the launch back through `sign_in::keys_wait` and phase one again
/// (`agents::start_after_gate`), unless this launch already waited once. Any other refusal,
/// and a second waited-out one, fails it with the refusal in force now
/// (`sign_in::launch_refusal`: desktop may have moved on since its gate answered), or with the
/// gate's code when none is.
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
        let refused = super::super::sign_in::launch_refusal(actor, thread)
            .unwrap_or_else(|| Error::refused(code, "Clauseo is not starting this agent for now"));
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
