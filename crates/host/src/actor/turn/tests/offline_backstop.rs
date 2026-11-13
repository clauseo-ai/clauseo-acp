//! The quiet backstop (`offline/quiet.rs`, D-050): a live turn whose agent sends no model output
//! for 5 minutes, with no tool call open and no ask, ends `Failed { agent.quiet }` whatever the
//! host's probes said, the answer so far kept and the queue held; output (a thought too) moves
//! it, an open tool call or an ask keeps it off, and an ask's end restarts the clock, so the
//! lawyer's time never counts. The probe is a fixed answer: no test here touches the network.

use std::time::Duration;

use clauseo_protocol::{AgentKind, ErrorCode, StopReason};
use serde_json::json;

use super::agents::send_prompt;
use super::offline::{answer, ended};
use super::offline_quiet::{
    CANCELLED, END_TURN, PERMISSION, PROMPT, allow, cancel, chunk, finish_turn, going, hold,
    probes, prompting_held, release_one, start, thought, tool, update, wait, wait_steps,
};
use crate::actor::turn::offline::probe::ReachProbe;
use crate::actor::turn::offline::quiet::QUIET_WAIT;
use crate::actor::turn::queue;

const SECOND: Duration = Duration::from_secs(1);

/// A second short of the quiet wait.
const ALMOST: Duration = Duration::from_secs(29);

#[test]
fn a_reached_probe_and_five_silent_minutes_end_the_turn_quiet() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        CANCELLED.to_owned(),
    ];
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, count) = start("backstop", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    let queued = send_prompt(&mut rig, 11, thread);
    // Every probe gets through, and the turn goes on to 4:59.
    wait_steps(&mut rig, thread, 9, QUIET_WAIT);
    wait(&mut rig, thread, ALMOST);
    assert!(going(&rig, thread));
    assert_eq!(probes(&count), 9);
    wait(&mut rig, thread, SECOND);
    finish_turn(&mut rig, thread);
    rig.settle_commits();
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentQuiet))
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    assert!(queue::queue_held(&rig.actor, thread), "the queue is held");
    assert_eq!(
        queue::queued_turns(&rig.actor, thread),
        [queued],
        "not sent"
    );
    rig.finish();
}

#[test]
fn a_long_think_streaming_thoughts_never_trips_the_limit() {
    let mut lines = vec![PROMPT.to_owned()];
    lines.extend((0..60).map(|n| thought(&format!("Weighing clause {n}. "))));
    lines.push(END_TURN.to_owned());
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, count) =
        start("backstop-thoughts", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 61);
    // A thought every 20 s for 20 minutes.
    for _ in 0..60 {
        release_one(&mut rig);
        wait(&mut rig, thread, Duration::from_secs(20));
    }
    assert!(going(&rig, thread));
    assert_eq!(probes(&count), 0);
    finish_turn(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::EndTurn), None)
    );
    rig.finish();
}

#[test]
fn an_open_tool_call_never_trips_the_limit() {
    let lines = [PROMPT.to_owned(), tool(), CANCELLED.to_owned()];
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, _) = start("backstop-tool", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    // Short of the open tool's 11-minute cap (10:59).
    wait_steps(&mut rig, thread, 21, QUIET_WAIT);
    wait(&mut rig, thread, ALMOST);
    assert!(going(&rig, thread));
    cancel(&mut rig, thread, turn);
    rig.finish();
}

/// Stage D's L1: a tool call left open keeps the arm and the backstop off only up to
/// `TOOL_LIMIT` (11 min, past Claude Bash's 10-minute timeout) with no word from the agent; a
/// tool update moves that, and past it the silence counts: 5 min of it are long gone, so the
/// backstop ends the turn at once.
#[test]
fn a_tool_call_open_with_no_word_for_eleven_minutes_ends_the_turn_quiet() {
    let update = update(
        &json!({"sessionUpdate": "tool_call_update", "toolCallId": "t-9",
        "status": "in_progress"}),
    );
    let lines = [PROMPT.to_owned(), tool(), update, CANCELLED.to_owned()];
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, count) =
        start("backstop-tool-cap", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 2);
    release_one(&mut rig);
    // A tool update at 9:00 moves the cap to 20:00: going at 19:59, ended at 20:00.
    wait_steps(&mut rig, thread, 18, QUIET_WAIT);
    release_one(&mut rig);
    wait_steps(&mut rig, thread, 21, QUIET_WAIT);
    wait(&mut rig, thread, ALMOST);
    assert!(going(&rig, thread));
    assert_eq!(probes(&count), 0);
    wait(&mut rig, thread, SECOND);
    finish_turn(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentQuiet))
    );
    rig.finish();
}

/// Stage D's L2: claude-agent-acp 0.89.1 sends a compaction's start at once and streams its
/// summary (`compaction_update`, `compaction_summary_chunk`), and each counts as output: a
/// compaction whose summary comes in over 6 minutes is never probed and never trips the backstop.
#[test]
fn a_compaction_streaming_its_summary_never_trips_the_limit() {
    let compaction = |status: &str| {
        update(
            &json!({"sessionUpdate": "compaction_update", "compactionId": "c-1",
            "status": status}),
        )
    };
    let summary = |n: usize| {
        update(
            &json!({"sessionUpdate": "compaction_summary_chunk", "compactionId": "c-1",
            "content": {"type": "text", "text": format!("Clause {n} was reviewed. ")}}),
        )
    };
    let mut lines = vec![PROMPT.to_owned(), compaction("in_progress")];
    lines.extend((0..14).map(summary));
    lines.push(compaction("completed"));
    lines.push(END_TURN.to_owned());
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, count) = start(
        "backstop-compaction",
        Some(AgentKind::Claude),
        &lines,
        probe,
    );
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 17);
    // The start, then a summary block every 25 s: 6:15 in all.
    for _ in 0..15 {
        release_one(&mut rig);
        wait(&mut rig, thread, Duration::from_secs(25));
    }
    assert!(going(&rig, thread));
    assert_eq!(probes(&count), 0);
    finish_turn(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::EndTurn), None)
    );
    rig.finish();
}

/// The lawyer answers a permission 10:15 in: the quiet clock runs from the answer, so the
/// first probe is at 10:45 and the backstop at 15:15, not at once.
#[test]
fn answering_a_permission_late_restarts_the_quiet_clock() {
    let lines = [
        PROMPT.to_owned(),
        PERMISSION.to_owned(),
        CANCELLED.to_owned(),
    ];
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, count) = start("backstop-ask", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    wait_steps(&mut rig, thread, 20, QUIET_WAIT);
    wait(&mut rig, thread, 15 * SECOND);
    assert_eq!(probes(&count), 0);
    allow(&mut rig, 11, thread);
    // 10:15 → 10:44: no probe yet; 10:45: the first.
    wait(&mut rig, thread, 15 * SECOND);
    wait(&mut rig, thread, 14 * SECOND);
    assert_eq!(probes(&count), 0);
    wait(&mut rig, thread, SECOND);
    assert_eq!(probes(&count), 1);
    // 15:14 still going; 15:15 the backstop.
    wait_steps(&mut rig, thread, 8, QUIET_WAIT);
    wait(&mut rig, thread, ALMOST);
    assert!(going(&rig, thread));
    wait(&mut rig, thread, SECOND);
    finish_turn(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentQuiet))
    );
    rig.finish();
}

/// One thought at 4:30 moves the end: still going at 5:00, ended at 9:30.
#[test]
fn a_thought_at_4_30_moves_the_backstop_to_9_30() {
    let lines = [
        PROMPT.to_owned(),
        thought("Weighing clause five."),
        CANCELLED.to_owned(),
    ];
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, _) = start("backstop-moved", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    wait_steps(&mut rig, thread, 9, QUIET_WAIT);
    release_one(&mut rig);
    wait(&mut rig, thread, QUIET_WAIT);
    assert!(going(&rig, thread), "still going at 5:00");
    wait_steps(&mut rig, thread, 8, QUIET_WAIT);
    wait(&mut rig, thread, ALMOST);
    assert!(going(&rig, thread), "still going at 9:29");
    wait(&mut rig, thread, SECOND);
    finish_turn(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentQuiet))
    );
    rig.finish();
}
