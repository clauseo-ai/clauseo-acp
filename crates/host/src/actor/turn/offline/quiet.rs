//! The quiet arm (D-050): a turn behind a proxy whose upstream is blocked can go silent with no
//! note at all (Claude retries out of sight; Codex retries without end and its notes may say a
//! server answered). So every live turn of a Claude or Codex agent keeps a clock of its last
//! model output (answer and thought chunks, tool calls and their updates, plans, compaction
//! updates, an ask), restarted when an ask ends, since the lawyer's time never counts. Whether
//! its service is known does not matter here: one that is not is never probed (it counts as
//! reached, `probe/service.rs`), so only the backstop ends such a turn. A tool call left open
//! keeps both off only while the agent sent something within [`TOOL_LIMIT`]; past it the
//! silence counts as any other. A compaction in progress (from its `in_progress` until it
//! ends, any other output, or the turn's end) holds the backstop off the same way, up to the
//! same cap, but not the probes: the summary call may wait long for its first token.
//! - [`QUIET_WAIT`] with no output, no open tool call and no ask: a quiet watch arms and the host
//!   probes the agent's service (`probe.rs`) at once. A probe that gets through waits again; two
//!   misses in a row ([`QUIET_MISSES`]) end the turn `agent.offline`, as any watch does. Output
//!   clears the watch; an agent's note turns it into a note's watch.
//! - [`QUIET_LIMIT`] with no output, no open tool call, no compaction and no ask ends the turn
//!   `Failed { agent.quiet }` (the answer so far kept, Try again, the queue held), whatever the
//!   probes said: the backstop for what a probe cannot see (a proxy that asks for credentials, a
//!   TLS-inspecting one that answers the handshake itself).
//!
//! - [`RETRY_LIMIT`] instead of [`QUIET_LIMIT`] while the agent said it waits to retry its
//!   service (an AIR `sessionFailure` of severity `warning`, category `connection` or
//!   `service`): such a wait can be minutes (Codex once waited 346 s with the model at capacity)
//!   and no field says how long. The clock restarts at each such word, so each new retry gives
//!   the full 15 minutes; the agent's next frame of any other kind ends the wait, restarting the
//!   clock once more, and the plain limit holds again. The host logs `AgentRetrying` with the
//!   agent's words at each retry, and `AgentRetryEnded` when the wait ends before the turn does
//!   (the turn's end clears it in the view on its own). Probes and a lost connection's watch run
//!   as they would.
//!
//! Off in unit tests unless a test turns it on (`TurnPath::quiet_watch`), like the real probe.

use std::time::Duration;

use clauseo_acp::{Origin, Retrying};
use clauseo_protocol::{AgentRetryEnded, AgentRetrying, DomainEvent, ErrorCode, ThreadId, TurnId};

use super::super::super::Actor;
use super::super::{TimerKind, TurnTimer, cancel, current, updates};
use super::{Arm, OFFLINE_WAIT, Watch, at_work, probe};
use crate::log_file::LIFECYCLE;

/// How long a turn may go with no model output, no open tool call and no ask before the host
/// probes the agent's service.
pub(in crate::actor::turn) const QUIET_WAIT: Duration = OFFLINE_WAIT;

/// The probes in a row a quiet watch may miss: the last ends the turn offline.
pub(in crate::actor::turn) const QUIET_MISSES: u8 = 2;

/// How long a turn may go with no model output, no open tool call and no ask at all: the
/// backstop ends it `agent.quiet`.
pub(in crate::actor::turn) const QUIET_LIMIT: Duration = Duration::from_secs(300);

/// How long a turn whose agent said it waits to retry its service may go with no word from it,
/// from that word: the backstop's limit while the wait lasts.
pub(in crate::actor::turn) const RETRY_LIMIT: Duration = Duration::from_mins(15);

/// How long an open tool call keeps the arm and the backstop off with no word from the agent
/// (its updates count): a minute past Claude Bash's own 600 s timeout, so a command that times
/// out at 10:00 ends with Claude's report of it, not `agent.quiet` a moment before.
pub(in crate::actor::turn) const TOOL_LIMIT: Duration = Duration::from_mins(11);

/// The quiet arm's clock of a live turn: when the agent last sent model output, and whether a
/// compaction of the turn is in progress.
#[derive(Debug)]
pub(in crate::actor::turn) struct QuietClock {
    /// When the agent last sent model output, or an ask of it ended, on the host's monotonic
    /// clock: the quiet arm runs from here.
    last_output: Duration,
    /// A compaction of the turn started and has not ended: it holds the backstop off, up to
    /// [`TOOL_LIMIT`] from `last_output`.
    compacting: bool,
    /// The agent said it waits to retry and has sent nothing else since: the backstop waits
    /// [`RETRY_LIMIT`] from `last_output`, which that word set.
    retrying: bool,
}

impl QuietClock {
    /// The clock of a turn that started at `started` (on the host's monotonic clock), with no
    /// compaction in progress.
    pub(in crate::actor::turn) fn started(started: Duration) -> Self {
        Self {
            last_output: started,
            compacting: false,
            retrying: false,
        }
    }
}

/// `thread`'s turn went live: its clock starts, and the quiet timer is armed when the switch is
/// on and the agent is of a known kind (its launch recorded a probe target, known or not).
pub(super) fn turn_started(actor: &mut Actor, thread: ThreadId) {
    let now = actor.now().mono;
    let Some(current) = current(actor, thread) else {
        return;
    };
    current.quiet.last_output = now;
    let turn = current.turn;
    if actor.turn_path.quiet_watch && probe::target(actor, thread).is_some() {
        arm(actor, thread, turn, QUIET_WAIT);
    }
}

/// The agent of `thread`'s live `turn` sent model output: its clock restarts.
pub(super) fn output(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let now = actor.now().mono;
    if let Some(current) =
        current(actor, thread).filter(|current| current.turn == turn && current.live())
    {
        current.quiet.last_output = now;
    }
}

/// Whether a compaction of `thread`'s turn is in progress.
pub(super) fn compacting(actor: &mut Actor, thread: ThreadId) -> bool {
    current(actor, thread).is_some_and(|current| current.quiet.compacting)
}

/// The agent of `thread`'s live `turn` sent output that leaves a compaction in progress, or
/// not: a compaction's start sets it; its end, or any output but its summary, clears it.
pub(super) fn set_compacting(actor: &mut Actor, thread: ThreadId, turn: TurnId, on: bool) {
    if let Some(current) =
        current(actor, thread).filter(|current| current.turn == turn && current.live())
    {
        current.quiet.compacting = on;
    }
}

/// An ask of `thread`'s agent ended (answered, cancelled, done): the clock of its live turn
/// restarts, so the lawyer's time never counts.
pub(in crate::actor::turn) fn ask_ended(actor: &mut Actor, thread: ThreadId) {
    let now = actor.now().mono;
    if let Some(current) = current(actor, thread).filter(|current| current.live()) {
        current.quiet.last_output = now;
    }
}

/// The agent of `thread` said it waits to retry its service, with `origin` when the word came
/// as an update of a known turn (a lost connection), else `None` (a `service` warning, which
/// names no turn). During the live turn, with no failure yet, the clock restarts, the wait
/// begins or goes on, and `AgentRetrying` logs the agent's words for the view.
pub(in crate::actor::turn) fn retrying(
    actor: &mut Actor,
    thread: ThreadId,
    origin: Option<Origin>,
    retry: &Retrying,
) {
    let now = actor.now().mono;
    let Some(current) =
        current(actor, thread).filter(|current| current.live() && current.failure.is_none())
    else {
        return;
    };
    let ours = match origin {
        Some(Origin::Turn(turn)) => turn == current.turn,
        Some(Origin::Late(_) | Origin::Replay) => false,
        Some(Origin::Idle) | None => true,
    };
    if !ours {
        return;
    }
    current.quiet.last_output = now;
    let again = std::mem::replace(&mut current.quiet.retrying, true);
    let turn = current.turn;
    tracing::info!(
        target: LIFECYCLE,
        %thread,
        by = "retry",
        again,
        "the agent waits to retry its service; the turn may stay quiet for 15 min"
    );
    let event = AgentRetrying {
        turn,
        title: retry.title.clone(),
        details: retry.details.clone(),
    };
    log(actor, thread, event.into());
}

/// The agent of `thread`'s live `turn` sent a frame other than a retry: a wait to retry ends,
/// the clock restarts (so the plain limit runs from the frame that ended the wait, not from
/// the last retry), and `AgentRetryEnded` tells the view. With no wait, nothing happens.
pub(in crate::actor::turn) fn retry_over(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let now = actor.now().mono;
    let Some(current) = current(actor, thread)
        .filter(|current| current.turn == turn && current.live() && current.quiet.retrying)
    else {
        return;
    };
    current.quiet.retrying = false;
    current.quiet.last_output = now;
    tracing::info!(target: LIFECYCLE, %thread, by = "retry", "the agent's wait to retry is over");
    log(actor, thread, AgentRetryEnded { turn }.into());
}

/// Logs `event` in `thread`'s stream; a failure is only logged.
fn log(actor: &mut Actor, thread: ThreadId, event: DomainEvent) {
    if let Err(error) = updates::commit(actor, thread, [event], Vec::new()) {
        let error = crate::log_file::Cause(&error);
        tracing::warn!(%thread, error = %error, "a retry's event was not logged");
    }
}

fn arm(actor: &mut Actor, thread: ThreadId, turn: TurnId, after: Duration) {
    let timer = TurnTimer {
        thread,
        kind: TimerKind::Quiet(turn),
    };
    let _handle = actor.arm_timer(after, timer);
}

/// The quiet timer of `thread`'s `turn` fired. A turn no longer live, or failed, stops it.
pub(in crate::actor::turn) fn on_timer(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let now = actor.now().mono;
    let (asking, working) = at_work(actor, thread, turn);
    let Some(current) = current(actor, thread)
        .filter(|current| current.turn == turn && current.live() && current.failure.is_none())
    else {
        return;
    };
    // Waiting on the lawyer is not quiet; the clock runs again from now.
    if asking {
        current.quiet.last_output = now;
        arm(actor, thread, turn, QUIET_WAIT);
        return;
    }
    let quiet = now.saturating_sub(current.quiet.last_output);
    // Nor is a tool call at work, while the agent sent something (its updates count) within
    // [`TOOL_LIMIT`]: one left open past it no longer keeps the arm or the backstop off.
    if working && quiet < TOOL_LIMIT {
        arm(
            actor,
            thread,
            turn,
            QUIET_WAIT.min(TOOL_LIMIT.saturating_sub(quiet)),
        );
        return;
    }
    // A wait to retry has its own limit; a compaction in progress holds the backstop (not the
    // probes) off up to the same cap as a tool call.
    let limit = if current.quiet.retrying {
        RETRY_LIMIT
    } else if current.quiet.compacting {
        TOOL_LIMIT
    } else {
        QUIET_LIMIT
    };
    if quiet >= limit {
        backstop(actor, thread, quiet, limit);
        return;
    }
    if quiet < QUIET_WAIT {
        arm(actor, thread, turn, QUIET_WAIT.saturating_sub(quiet));
        return;
    }
    let next = QUIET_WAIT.min(limit.saturating_sub(quiet));
    // A standing watch keeps its own timer.
    if current.offline.is_none() {
        let mut watch = Watch::new(now, Arm::Quiet);
        watch.probing = true;
        current.offline = Some(watch);
        tracing::info!(
            target: LIFECYCLE,
            %thread,
            by = "quiet",
            quiet_s = quiet.as_secs(),
            "the agent sent nothing for 30 s; the host probes its service"
        );
        probe::start(actor, thread, turn);
    }
    arm(actor, thread, turn, next);
}

/// Cancels `thread`'s live turn with `agent.quiet` as its failure, `limit` being the silence
/// that ended it. A watch standing stays for the end to read (its held lines go back), and a
/// probe's answer after this is ignored.
fn backstop(actor: &mut Actor, thread: ThreadId, quiet: Duration, limit: Duration) {
    let Some(current) = current(actor, thread) else {
        return;
    };
    let minutes = limit.as_secs() / 60;
    let last = current.offline.as_ref().and_then(super::Watch::class);
    let reached = current.offline.as_ref().is_some_and(|watch| watch.reached);
    tracing::warn!(
        target: LIFECYCLE,
        %thread,
        by = "backstop",
        quiet_s = quiet.as_secs(),
        limit_s = limit.as_secs(),
        class = last.map_or("none", probe::ReachClass::as_str),
        reached,
        "the agent sent nothing for {} min; ending the turn",
        minutes
    );
    current.failure = Some((
        ErrorCode::AgentQuiet,
        Some(format!(
            "the agent sent nothing for {minutes} min with no tool running and no question waiting"
        )),
    ));
    cancel::cancel_running(actor, thread, None, cancel::why::FAILURE);
}
