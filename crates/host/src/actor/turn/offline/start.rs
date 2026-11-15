//! A launch that failed on its connection (`agent.start_offline`): the agent answered a start
//! request (`initialize`, `session/new`, `session/load`) with an error whose text names a
//! connection failure and no server's answer ([`classify`]). The text alone never says offline:
//! a model's words or an adapter's message can name a connection for another reason. So the
//! turn waits for the host's own probe of the agent's service (`probe.rs`), and ends
//! `agent.start_offline` only when that does not get through either; a probe that gets through,
//! an agent whose service cannot be known, or a probe that could not run ends it
//! `agent.launch_failed`, as any other failed start. The log gets fixed fields only.

use clauseo_protocol::{ErrorCode, ThreadId, TurnId};

use super::super::super::Actor;
use super::super::current;
use super::probe::{Reach, ReachClass, Target};
use super::{TurnPhase, classify, probe};
use crate::error::Error;
use crate::log_file::LIFECYCLE;

/// A failed start waiting for the probe: its turn and the detail its `TurnFailed` gets.
#[derive(Debug)]
pub(in crate::actor::turn) struct StartProbe {
    turn: TurnId,
    detail: Option<String>,
}

/// Whether `error`, a failed start's, names a connection failure and no server's answer: an
/// error the agent answered with ([`clauseo_acp::Error::Rpc`]), its text or its `data` read.
fn says_offline(error: &Error) -> bool {
    let Error::Acp(acp @ clauseo_acp::Error::Rpc { .. }) = error else {
        return false;
    };
    let text = acp.to_string();
    let texts = [Some(text.as_str()), acp.rpc_data()];
    let texts = || texts.iter().flatten();
    !texts().any(|text| classify::says_server(text))
        && texts().any(|text| classify::says_connection(text))
}

/// Starts the probe for `thread`'s failed start of `turn` when `error` says offline and the
/// agent's service is known, and keeps the turn waiting for it: `true`. Else `false`, and the
/// caller ends the turn `agent.launch_failed` now. Called while the failed agent is still the
/// thread's, so the target its launch recorded is there.
pub(in crate::actor::turn) fn probe_first(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    error: &Error,
    detail: Option<String>,
) -> bool {
    if !says_offline(error) {
        return false;
    }
    if !matches!(probe::target(actor, thread), Some(Target::Service(_))) {
        tracing::info!(%thread, "a start failed on its connection; the service is unknown");
        return false;
    }
    tracing::info!(%thread, %turn, "a start failed on its connection; probing the service");
    let waiting = StartProbe { turn, detail };
    actor.turn_path.start_probes.insert(thread, waiting);
    probe::start(actor, thread, turn);
    true
}

/// The probe for `thread`'s `turn` answered: when a failed start waits for it, its turn ends
/// `agent.start_offline` if the service was not reached, else `agent.launch_failed`, and
/// `true`. `false` when no failed start of that turn waits (the answer is a watch's). A turn
/// that was cancelled or ended meanwhile is left as it is.
pub(super) fn on_probed(actor: &mut Actor, thread: ThreadId, turn: TurnId, reach: Reach) -> bool {
    let probes = &mut actor.turn_path.start_probes;
    if probes
        .get(&thread)
        .is_none_or(|waiting| waiting.turn != turn)
    {
        return false;
    }
    let Some(StartProbe { detail, .. }) = probes.remove(&thread) else {
        return false;
    };
    let offline = !reach.reached() && reach.class != ReachClass::NoService;
    tracing::info!(
        target: LIFECYCLE,
        %thread,
        %turn,
        offline,
        class = reach.class.as_str(),
        via = reach.via.as_str(),
        ms = reach.ms,
        "the host's probe after a start failed on its connection"
    );
    let live = current(actor, thread).is_some_and(|current| {
        current.turn == turn && current.phase == TurnPhase::Launching && !current.cancelled
    });
    if live {
        let code = if offline {
            ErrorCode::AgentStartOffline
        } else {
            ErrorCode::AgentLaunchFailed
        };
        super::super::steps::end_launch_failed(actor, thread, turn, code, detail);
    }
    true
}
