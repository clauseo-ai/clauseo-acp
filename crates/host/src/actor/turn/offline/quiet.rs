//! The quiet arm (D-050): a turn behind a proxy whose upstream is blocked can go silent with no
//! note at all (Claude retries out of sight; Codex retries without end and its notes may say a
//! server answered). So every live turn of a Claude or Codex agent keeps a clock of its last
//! model output (answer and thought chunks, tool calls and their updates, plans, compaction
//! updates, an ask), restarted when an ask ends, since the lawyer's time never counts. Whether
//! its service is known does not matter here: one that is not is never probed (it counts as
//! reached, `probe/service.rs`), so only the backstop ends such a turn. A tool call left open
//! keeps both off only while the agent sent something within [`TOOL_LIMIT`]; past it the
//! silence counts as any other.
//! - [`QUIET_WAIT`] with no output, no open tool call and no ask: a quiet watch arms and the host
//!   probes the agent's service (`probe.rs`) at once. A probe that gets through waits again; two
//!   misses in a row ([`QUIET_MISSES`]) end the turn `agent.offline`, as any watch does. Output
//!   clears the watch; an agent's note turns it into a note's watch.
//! - [`QUIET_LIMIT`] with no output, no open tool call and no ask ends the turn
//!   `Failed { agent.quiet }` (the answer so far kept, Try again, the queue held), whatever the
//!   probes said: the backstop for what a probe cannot see (a proxy that asks for credentials, a
//!   TLS-inspecting one that answers the handshake itself).
//!
//! Off in unit tests unless a test turns it on (`TurnPath::quiet_watch`), like the real probe.

use std::time::Duration;

use clauseo_protocol::{ErrorCode, ThreadId, TurnId};

use super::super::super::Actor;
use super::super::{TimerKind, TurnTimer, cancel, current};
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

/// How long an open tool call keeps the arm and the backstop off with no word from the agent
/// (its updates count): a minute past Claude Bash's own 600 s timeout, so a command that times
/// out at 10:00 ends with Claude's report of it, not `agent.quiet` a moment before.
pub(in crate::actor::turn) const TOOL_LIMIT: Duration = Duration::from_mins(11);

/// The backstop's detail (the event log, never shown).
const LIMIT_DETAIL: &str =
    "the agent sent nothing for 5 min with no tool running and no question waiting";

/// `thread`'s turn went live: its clock starts, and the quiet timer is armed when the switch is
/// on and the agent is of a known kind (its launch recorded a probe target, known or not).
pub(super) fn turn_started(actor: &mut Actor, thread: ThreadId) {
    let now = actor.now().mono;
    let Some(current) = current(actor, thread) else {
        return;
    };
    current.last_output = now;
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
        current.last_output = now;
    }
}

/// An ask of `thread`'s agent ended (answered, cancelled, done): the clock of its live turn
/// restarts, so the lawyer's time never counts.
pub(in crate::actor::turn) fn ask_ended(actor: &mut Actor, thread: ThreadId) {
    let now = actor.now().mono;
    if let Some(current) = current(actor, thread).filter(|current| current.live()) {
        current.last_output = now;
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
        current.last_output = now;
        arm(actor, thread, turn, QUIET_WAIT);
        return;
    }
    let quiet = now.saturating_sub(current.last_output);
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
    if quiet >= QUIET_LIMIT {
        backstop(actor, thread, quiet);
        return;
    }
    if quiet < QUIET_WAIT {
        arm(actor, thread, turn, QUIET_WAIT.saturating_sub(quiet));
        return;
    }
    let next = QUIET_WAIT.min(QUIET_LIMIT.saturating_sub(quiet));
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

/// Cancels `thread`'s live turn with `agent.quiet` as its failure. A watch standing stays for
/// the end to read (its held lines go back), and a probe's answer after this is ignored.
fn backstop(actor: &mut Actor, thread: ThreadId, quiet: Duration) {
    let Some(current) = current(actor, thread) else {
        return;
    };
    let last = current.offline.as_ref().and_then(|watch| watch.class);
    let reached = current.offline.as_ref().is_some_and(|watch| watch.reached);
    tracing::warn!(
        target: LIFECYCLE,
        %thread,
        by = "backstop",
        quiet_s = quiet.as_secs(),
        class = last.map_or("none", probe::ReachClass::as_str),
        reached,
        "the agent sent nothing for 5 min; ending the turn"
    );
    current.failure = Some((ErrorCode::AgentQuiet, Some(LIMIT_DETAIL.to_owned())));
    cancel::cancel_running(actor, thread, None, cancel::why::FAILURE);
}
