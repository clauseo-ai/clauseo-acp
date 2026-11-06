//! Desktop's launch gate (R17 M3, `HostConfig::with_launch_gate`): a cheap look at an agent's
//! shared login file just before each launch, so a login changed since desktop's last poll is
//! caught before the agent starts rather than up to a poll later. Desktop holds the refusal
//! facts: the host never writes the gate's code into the launch refusals
//! (`agent/launch_refusals.rs`), it only acts on the answer (`actor/turn/launch_checks/gate.rs`).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use clauseo_protocol::{AgentId, ErrorCode};

use crate::error::Error;

/// Desktop's launch gate: asked with the agent's id just before each of its launches, of any
/// agent kind, it answers whether that launch must not start now.
///
/// It must be cheap and must not block: a stat of the login file (its size, its modified time,
/// its file id), never an open of a file's contents, never a status run of the agent, and no
/// lock held across I/O. It runs off the actor, on the launch's `clauseo-codex-fs` prepare
/// thread beside the data-folder refusal, never on the host's actor; a gate that hangs is
/// bounded by that prepare job's wait, and the launch then fails with `agent.launch_failed`,
/// which may be retried.
///
/// Its answer:
/// - `None`: the launch goes on as without a gate.
/// - `Some(code)`, a code `HostHandle::set_launch_refused` takes: desktop has already set that
///   refusal itself, and awaited the call, before the gate returns; the host never writes it.
///   A code the launch waits out (`agent.identity_checking`, say) drops this prepared launch,
///   which then waits, at most the launch refusals' 2 s and woken early when desktop lifts or
///   changes the refusal, asks desktop's launch retry with the code, and prepares again,
///   asking the gate again. Lifted already, it prepares again at once. One wait per launch: a
///   second waited-out answer fails the launch with that code, and so does a refusal still in
///   force after the wait. Any other refusal fails the launch with its code.
/// - `Some(code)` with a code that is not a launch refusal: the launch fails closed with
///   `agent.launch_failed`, and a warning names the agent and the code.
/// - A panic fails closed the same way, with a warning that the launch gate panicked.
pub type LaunchGate = Arc<dyn Fn(&AgentId) -> Option<ErrorCode> + Send + Sync>;

/// Asks `gate` about `agent`, on the prepare thread: its answer, or `agent.launch_failed`
/// when it panicked. No gate answers `None`.
pub(crate) fn ask(gate: Option<&LaunchGate>, agent: &AgentId) -> Result<Option<ErrorCode>, Error> {
    let Some(gate) = gate else {
        return Ok(None);
    };
    catch_unwind(AssertUnwindSafe(|| gate(agent))).map_err(|_panic| {
        tracing::warn!(%agent, "the launch gate panicked; the launch fails");
        failed()
    })
}

/// The failure of a launch the gate closed: a panic, or a code that is not a launch refusal.
pub(crate) fn failed() -> Error {
    Error::refused(
        ErrorCode::AgentLaunchFailed,
        "Clauseo couldn't check the agent's sign-in before starting it",
    )
}
