//! The truth of an offline end: "You're offline" (`agent.offline`) only while the window says
//! the computer is offline and the host's probe of the agent's service cannot get through
//! either. Else a connection failure is the service's, or what stands in front of it (a proxy,
//! a firewall, TLS inspection): `agent.unreachable`. A request that timed out while the probe
//! reached the service is a slow service: `agent.quiet` ("… stopped answering").
//!
//! A failed prompt ([`super::ended`]) reads the turn's last probe when it is younger than
//! [`OFFLINE_WAIT`]; else its end waits for one fresh probe, at most [`PROBE_CAP`]
//! ([`hold_for_probe`]). A probe that never answers in that time counts as a miss.

use std::time::Duration;

use clauseo_acp::PromptEnd;
use clauseo_protocol::{ErrorCode, ThreadId, TurnId};

use super::super::super::Actor;
use super::super::{TurnPhase, current, end};
use super::probe::{self, PROBE_CAP, Reach};
use super::{OFFLINE_WAIT, arm_timer_for, ended};

/// A prompt's end held for the host's probe, and the probe's answer once it came.
#[derive(Debug)]
pub(in crate::actor::turn) struct Hold {
    /// The prompt's result, until the probe answers or [`PROBE_CAP`] runs out.
    result: Option<Result<PromptEnd, clauseo_acp::Error>>,
    /// The probe's answer, for the end to read.
    reach: Option<Reach>,
    /// When the end was held, on the host's monotonic clock.
    since: Duration,
}

/// The prompt of `thread`'s `turn` returned `result`. When it failed for its connection (as
/// [`ended::candidate`] reads it), the agent's service can be probed and no probe of the turn
/// answered within [`OFFLINE_WAIT`], its end waits for one: `None`, and the probe's answer (or
/// [`PROBE_CAP`]) hands the result on to `end::on_prompt_done`. Else the result, at once.
pub(in crate::actor::turn) fn hold_for_probe(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    result: Result<PromptEnd, clauseo_acp::Error>,
) -> Option<Result<PromptEnd, clauseo_acp::Error>> {
    let open = current(actor, thread).is_some_and(|current| {
        current.turn == turn
            && current.phase == TurnPhase::Prompting
            && !current.cancelled
            && current.probe_hold.is_none()
    });
    if !open
        || probe::target(actor, thread).is_none()
        || fresh_reach(actor, thread).is_some()
        || ended::candidate(actor, thread, &result).is_none()
    {
        return Some(result);
    }
    let now = actor.now().mono;
    let current = current(actor, thread)?;
    let probing = current.offline.as_ref().is_some_and(|watch| watch.probing);
    current.probe_hold = Some(Hold {
        result: Some(result),
        reach: None,
        since: now,
    });
    tracing::info!(%thread, probing, "a prompt failed for its connection; the end waits for a probe");
    arm_timer_for(actor, thread, turn, PROBE_CAP);
    // A probe already on its way answers for both.
    if !probing {
        probe::start(actor, thread, turn);
    }
    None
}

/// The host's probe for `thread`'s `turn` answered: a held end takes it (the watch too, when
/// one stands) and goes on. Whether there was a held end.
pub(super) fn on_probed(actor: &mut Actor, thread: ThreadId, turn: TurnId, reach: Reach) -> bool {
    let now = actor.now().mono;
    let Some(current) = current(actor, thread).filter(|current| current.turn == turn) else {
        return false;
    };
    let Some(hold) = current
        .probe_hold
        .as_mut()
        .filter(|hold| hold.result.is_some())
    else {
        return false;
    };
    hold.reach = Some(reach);
    if let Some(watch) = current.offline.as_mut() {
        watch.probing = false;
        watch.probed = Some((now, reach));
        watch.reached = reach.reached();
    }
    release(actor, thread, turn);
    true
}

/// An offline timer of `thread`'s `turn` fired: while an end is held, only [`PROBE_CAP`] counts
/// (a watch's own timer waits), and once it runs out the end goes on with no probe's answer,
/// a miss. Whether an end is held.
pub(super) fn on_timer(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> bool {
    let now = actor.now().mono;
    let Some(current) = current(actor, thread).filter(|current| current.turn == turn) else {
        return false;
    };
    let Some(since) = current
        .probe_hold
        .as_ref()
        .filter(|hold| hold.result.is_some())
        .map(|hold| hold.since)
    else {
        return false;
    };
    if now.saturating_sub(since) >= PROBE_CAP {
        tracing::info!(%thread, "the probe did not answer in time; a miss");
        release(actor, thread, turn);
    }
    true
}

/// Hands `thread`'s held result on to the prompt's end.
fn release(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let result = current(actor, thread)
        .and_then(|current| current.probe_hold.as_mut())
        .and_then(|hold| hold.result.take());
    if let Some(result) = result {
        end::on_prompt_done(actor, thread, turn, result);
    }
}

/// The probe's answer for `thread`'s turn the verdict reads: a held end's, else the watch's
/// last when younger than [`OFFLINE_WAIT`].
pub(super) fn fresh_reach(actor: &mut Actor, thread: ThreadId) -> Option<Reach> {
    let now = actor.now().mono;
    let current = current(actor, thread)?;
    if let Some(reach) = current.probe_hold.as_ref().and_then(|hold| hold.reach) {
        return Some(reach);
    }
    current
        .offline
        .as_ref()
        .and_then(|watch| watch.probed)
        .filter(|(at, _)| now.saturating_sub(*at) < OFFLINE_WAIT)
        .map(|(_, reach)| reach)
}

/// The code a connection failure of `thread`'s turn ends with (the module doc): offline only
/// while the window says so and the probe missed (a launch with nothing to probe counts as a
/// miss then, as for the window's watch); a request that `timed_out` while the probe reached
/// the service is quiet; anything else is unreachable.
pub(super) fn verdict(actor: &mut Actor, thread: ThreadId, timed_out: bool) -> ErrorCode {
    let window = actor.turn_path.offline;
    let reach = fresh_reach(actor, thread);
    let reached = reach.is_some_and(|reach| reach.reached() && !(window && reach.class.unknown()));
    let code = if window && !reached {
        ErrorCode::AgentOffline
    } else if reached && timed_out {
        ErrorCode::AgentQuiet
    } else {
        ErrorCode::AgentUnreachable
    };
    let class = reach.map_or("none", |reach| reach.class.as_str());
    tracing::info!(%thread, window_offline = window, reached, timed_out, class, ?code, "a connection failure's verdict");
    code
}

/// `detail` with the probe's fixed fields, for firm IT (never a host or bytes).
pub(super) fn with_probe(detail: &str, reach: Option<Reach>) -> String {
    match reach {
        Some(reach) => format!(
            "{detail}; probe class={} via={} status={}",
            reach.class.as_str(),
            reach.via.as_str(),
            reach.status
        ),
        None => format!("{detail}; no probe answered"),
    }
}
