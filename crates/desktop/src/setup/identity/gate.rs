//! The host's launch gate (`HostConfig::with_launch_gate`, R17 review M3): a stat of the
//! agent's login files at each launch, on the host's prepare thread, so a swap made inside the
//! watcher's [`super::POLL`] is seen before the launch goes on.
//!
//! - **Fresh stamps** (those the verdict holds for): `None` at once, no other work; the launch
//!   costs one stat per login file.
//! - **A difference** (or, with an account recorded, no stamps held): the verdict becomes due,
//!   a check in flight is dropped and a new one asked for, and `agent.identity_checking` is put
//!   on the agent's launches (`set_launch_refused`, awaited) before the gate answers it. The
//!   host then waits that refusal out (up to its 2 s, woken when the check lifts it), and asks
//!   for the check again through its launch retry.
//! - **An identity refusal already in force** (due, checking, unknown, changed): the gate answers
//!   it, the host's table holding it first.
//! - **A key login** (`exempt`): `None`.
//! - **A pending sign-in through Clauseo**: stamps that differ from those the sign-in took make
//!   a verdict that refuses nothing due (`agent.identity_checking`), held until its record
//!   lands (R17 review M1).
//! - **The followers dead** (`setup/supervise.rs`): `agent.identity_unknown` for every agent on
//!   the shared login.
//!
//! **It must not panic:** in release builds a panic aborts the process (`panic = "abort"`), so
//! the stat path has no `unwrap`, `expect` or indexing; a stat that fails is a stamp of its
//! own (`Stamp::Unreadable`), which differs from the one held and so checks.
//!
//! A launch that passes marks the agent launched, after which an account with no record is
//! never trusted on first use (`Identities::judge`). Never an open, never a status run; the
//! identity lock is held for no I/O.

use std::sync::{Arc, OnceLock};

use clauseo_host::LaunchGate;
use clauseo_protocol::{AgentId, ErrorCode};

use super::Verdict;
use super::check::exempt;
use super::watch::stamps;
use crate::setup::backend::AgentsBackend;
use crate::setup::refusal;

/// The codes the gate answers: the identity check's.
const IDENTITY_CODES: [ErrorCode; 3] = [
    ErrorCode::AgentIdentityChecking,
    ErrorCode::AgentIdentityUnknown,
    ErrorCode::SigninAccountChanged,
];

/// The gate's answer for a launch of `agent`, as the module docs say. Blocks the host's prepare
/// thread only to tell the host the refusal it answers.
pub(crate) fn gate(backend: &AgentsBackend, agent: &AgentId) -> Option<ErrorCode> {
    let identity = &backend.wiring.identity;
    if exempt(backend, agent) {
        identity.entry(agent, |entry| entry.launched = true);
        return None;
    }
    let stamps = stamps(backend, agent);
    let recorded = identity.recorded_now(agent);
    let now = backend.compose.monotonic();
    let moved = identity.entry(agent, |entry| {
        if entry.expecting(now) {
            // A pending sign-in's files moved: refused until its record lands (M1).
            return entry.hold_if_moved(&stamps);
        }
        let rests = matches!(
            entry.verdict,
            Verdict::Idle | Verdict::Same | Verdict::SignedOut
        );
        if !rests {
            return false;
        }
        let moved = entry
            .stamps
            .as_ref()
            .map_or(recorded, |before| *before != stamps);
        if moved {
            entry.verdict = Verdict::Due;
            entry.generation += 1;
        }
        moved
    });
    if moved {
        tracing::info!(%agent, kind = "login_changed_at_launch", "identity");
        identity.ask(agent);
    }
    let code = refusal::wanted(backend, agent).filter(|code| IDENTITY_CODES.contains(code));
    if let Some(code) = code {
        futures::executor::block_on(refusal::sync(backend, agent));
        return Some(code);
    }
    identity.entry(agent, |entry| entry.launched = true);
    None
}

/// Where the setup thread puts the backend once it is up, for the launch gate the host was
/// given before: until then the gate answers `None` (the host refuses `agent.keys_loading`
/// meanwhile, until setup tells it each agent's refusal).
pub(crate) type GateSlot = Arc<OnceLock<Arc<AgentsBackend>>>;

/// The host's launch gate over `slot`: [`gate`] once the backend is there.
pub(crate) fn launch_gate(slot: &GateSlot) -> LaunchGate {
    let slot = Arc::clone(slot);
    Arc::new(move |agent: &AgentId| slot.get().and_then(|backend| gate(backend, agent)))
}
