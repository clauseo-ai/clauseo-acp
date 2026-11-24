//! An agent that says it waits to retry its service (`offline/quiet.rs`): an AIR
//! `sessionFailure` of severity `warning`, category `connection` or `service`, as codex-acp
//! 2.10.0 and claude-agent-acp 0.110.1 send it. From that word the turn may stay silent 15
//! minutes, not 5; each new retry restarts the 15; the agent's next frame of another kind ends
//! the wait and the plain 5 minutes run from it. The view's header shows the wait (`retrying`,
//! with the agent's title and details) from `AgentRetrying` until `AgentRetryEnded` or the
//! turn's end. The probe is a fixed answer that gets through: no test here touches the network.

use std::time::Duration;

use clauseo_protocol::{AgentKind, AgentRetry, DomainEvent, ErrorCode, StopReason, StreamId};
use clauseo_reducer::StreamState;
use serde_json::{Value, json};

use super::offline::{answer, ended};
use super::offline_quiet::{
    CANCELLED, END_TURN, PROMPT, chunk, finish_turn, going, hold, prompting_held, release_one,
    start, update, wait, wait_steps,
};
use super::rig::Rig;
use crate::actor::turn::offline::probe::ReachProbe;
use crate::actor::turn::offline::quiet::QUIET_WAIT;
use clauseo_protocol::ThreadId;

const SECOND: Duration = Duration::from_secs(1);

/// A second short of the quiet wait.
const ALMOST: Duration = Duration::from_secs(29);

/// codex-acp 2.10.0's retry after `OpenAI` refused the request mid-answer.
const CODEX_TITLE: &str = "Reconnecting... 1/5";
const CODEX_CAPACITY: &str = "Selected model is at capacity. Please try a different model.";
const CODEX_DROPPED: &str = "stream disconnected before completion: Connection reset by peer";

/// claude-agent-acp 0.110.1's overload retry.
const CLAUDE_TITLE: &str = "Retrying Claude, attempt 2 of 10.";
const CLAUDE_DETAILS: &str = "HTTP 529 (overloaded). Next attempt in 8s.";

/// A `session_info_update` whose `sessionFailure` says the agent waits to retry.
fn retry(category: &str, title: &str, details: &str) -> String {
    let meta: Value = json!({"jetbrains": {"air": {"version": 1, "sessionFailure": {
        "id": "019b-turn:error", "revision": 1, "category": category, "severity": "warning",
        "title": title, "details": details, "actions": [],
    }}}});
    update(&json!({"sessionUpdate": "session_info_update", "_meta": meta}))
}

/// A `usage_update`: a frame of another kind that is no model output.
fn usage() -> String {
    update(&json!({"sessionUpdate": "usage_update", "used": 1200, "size": 200_000}))
}

/// The header's `retrying` as the host's loaded view has it, once commits settle.
fn retrying(rig: &mut Rig, thread: ThreadId) -> Option<AgentRetry> {
    rig.settle_commits();
    let loaded = rig.actor.loaded(StreamId::Thread(thread))?;
    let StreamState::Thread(state) = &loaded.state else {
        return None;
    };
    state.header()?.retrying.clone()
}

/// The retry events of `log`, in order: `Some(title)` for `AgentRetrying`, `None` for
/// `AgentRetryEnded`.
fn retry_events(log: &[DomainEvent]) -> Vec<Option<String>> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::AgentRetrying(retrying) = event {
                Some(retrying.title.clone())
            } else if let DomainEvent::AgentRetryEnded(_) = event {
                Some(None)
            } else {
                None
            }
        })
        .collect()
}

/// The detail of the log's `TurnFailed`.
fn failure_detail(log: &[DomainEvent]) -> String {
    log.iter()
        .find_map(|event| {
            let DomainEvent::TurnFailed(failed) = event else {
                return None;
            };
            failed.detail.clone()
        })
        .expect("a TurnFailed with a detail")
}

/// (a) A retry mid-answer, then 6 silent minutes, then the answer goes on: the turn survives,
/// the header shows the wait with the agent's words and drops it at the next chunk.
fn survives_six_minutes(test: &str, kind: AgentKind, frame: &str, title: &str, details: &str) {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        frame.to_owned(),
        chunk(" the buyer pays."),
        END_TURN.to_owned(),
    ];
    let (mut rig, thread, _) = start(test, Some(kind), &lines, ReachProbe::fixed(true));
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 4);
    release_one(&mut rig);
    assert_eq!(retrying(&mut rig, thread), None, "no wait before the retry");
    release_one(&mut rig);
    let shown = retrying(&mut rig, thread).expect("the wait shows");
    assert_eq!(shown.title.as_deref(), Some(title));
    assert_eq!(shown.details.as_deref(), Some(details));
    // Six silent minutes, past the plain 5: still going, still shown.
    wait_steps(&mut rig, thread, 12, QUIET_WAIT);
    assert!(going(&rig, thread), "still going at 6:00");
    assert!(retrying(&mut rig, thread).is_some(), "still shown at 6:00");
    release_one(&mut rig);
    assert_eq!(
        retrying(&mut rig, thread),
        None,
        "the next chunk ends the wait"
    );
    finish_turn(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(ended(&log, turn), (Some(StopReason::EndTurn), None));
    assert_eq!(answer(&log, turn), "Clause five says the buyer pays.");
    assert_eq!(retry_events(&log), [Some(title.to_owned()), None]);
    rig.finish();
}

#[test]
fn a_codex_at_capacity_retry_and_six_silent_minutes_leave_the_turn_going() {
    let frame = retry("service", CODEX_TITLE, CODEX_CAPACITY);
    let test = "retry-codex-capacity";
    survives_six_minutes(test, AgentKind::Codex, &frame, CODEX_TITLE, CODEX_CAPACITY);
}

/// A dropped stream is also a lost connection: its watch and probes run as before, and the
/// probe that gets through lets the longer limit hold.
#[test]
fn a_codex_dropped_stream_retry_and_six_silent_minutes_leave_the_turn_going() {
    let frame = retry("connection", CODEX_TITLE, CODEX_DROPPED);
    let test = "retry-codex-dropped";
    survives_six_minutes(test, AgentKind::Codex, &frame, CODEX_TITLE, CODEX_DROPPED);
}

#[test]
fn a_claude_overload_retry_and_six_silent_minutes_leave_the_turn_going() {
    let frame = retry("service", CLAUDE_TITLE, CLAUDE_DETAILS);
    let test = "retry-claude-overload";
    survives_six_minutes(
        test,
        AgentKind::Claude,
        &frame,
        CLAUDE_TITLE,
        CLAUDE_DETAILS,
    );
}

/// Runs `thread`'s silent turn to `minutes` less a second (asserting it goes on), then the last
/// second, and returns the log once the turn has ended.
fn silent_until(rig: &mut Rig, thread: ThreadId, minutes: u32) -> Vec<DomainEvent> {
    wait_steps(rig, thread, minutes * 2 - 1, QUIET_WAIT);
    wait(rig, thread, ALMOST);
    assert!(
        going(rig, thread),
        "still going a second short of {minutes} min"
    );
    wait(rig, thread, SECOND);
    finish_turn(rig, thread);
    rig.log(thread)
}

/// (b) A retry, then 15 silent minutes: the backstop ends the turn `agent.quiet` as it would
/// after 5 plain ones, its detail naming the 15, and the turn's end clears the wait.
#[test]
fn a_retry_then_fifteen_silent_minutes_end_the_turn_quiet() {
    let lines = [
        PROMPT.to_owned(),
        retry("service", CODEX_TITLE, CODEX_CAPACITY),
        CANCELLED.to_owned(),
    ];
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, _) = start("retry-limit", Some(AgentKind::Codex), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    assert!(retrying(&mut rig, thread).is_some());
    let log = silent_until(&mut rig, thread, 15);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentQuiet))
    );
    let detail = failure_detail(&log);
    assert!(
        detail.contains("for 15 min "),
        "the limit applied: {detail}"
    );
    assert_eq!(
        retrying(&mut rig, thread),
        None,
        "the turn's end clears the wait"
    );
    assert_eq!(retry_events(&log), [Some(CODEX_TITLE.to_owned())]);
    rig.finish();
}

/// (c) No retry, 5 silent minutes: the plain backstop, unchanged (a Codex turn here; Claude's
/// is `offline_backstop.rs`).
#[test]
fn no_retry_and_five_silent_minutes_end_the_turn_quiet() {
    let lines = [PROMPT.to_owned(), CANCELLED.to_owned()];
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, _) = start("retry-none", Some(AgentKind::Codex), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    let log = silent_until(&mut rig, thread, 5);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentQuiet))
    );
    assert!(failure_detail(&log).contains("for 5 min "));
    assert_eq!(retry_events(&log), Vec::<Option<String>>::new());
    rig.finish();
}

/// (d) A second retry 10 minutes after the first restarts the 15: going at 24:59, ended at
/// 25:00. The header keeps the wait's start and takes the second retry's words.
#[test]
fn a_second_retry_restarts_the_fifteen_minutes() {
    let second = "Retrying Claude, attempt 3 of 10.";
    let lines = [
        PROMPT.to_owned(),
        retry("service", CLAUDE_TITLE, CLAUDE_DETAILS),
        retry(
            "service",
            second,
            "HTTP 529 (overloaded). Next attempt in 16s.",
        ),
        CANCELLED.to_owned(),
    ];
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, _) = start("retry-again", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 2);
    release_one(&mut rig);
    let first = retrying(&mut rig, thread).expect("the wait shows");
    wait_steps(&mut rig, thread, 20, QUIET_WAIT);
    assert!(going(&rig, thread), "still going at 10:00");
    release_one(&mut rig);
    let shown = retrying(&mut rig, thread).expect("the wait still shows");
    assert_eq!(shown.since, first.since, "one wait, from its first retry");
    assert_eq!(shown.title.as_deref(), Some(second));
    // 10:00 + 15:00.
    let log = silent_until(&mut rig, thread, 15);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentQuiet))
    );
    assert_eq!(
        retry_events(&log),
        [Some(CLAUDE_TITLE.to_owned()), Some(second.to_owned())]
    );
    rig.finish();
}

/// A frame of another kind that is no model output (usage) still ends the wait: the plain 5
/// minutes run again, from that frame. Retry at 0:00, usage at 6:00, going at 10:59, ended at
/// 11:00 on the plain limit.
#[test]
fn any_other_frame_ends_the_wait_and_the_plain_limit_runs_from_it() {
    let lines = [
        PROMPT.to_owned(),
        retry("service", CLAUDE_TITLE, CLAUDE_DETAILS),
        usage(),
        CANCELLED.to_owned(),
    ];
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, _) = start("retry-over", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 2);
    release_one(&mut rig);
    wait_steps(&mut rig, thread, 12, QUIET_WAIT);
    assert!(going(&rig, thread), "still going at 6:00");
    release_one(&mut rig);
    assert_eq!(retrying(&mut rig, thread), None, "the usage ends the wait");
    let log = silent_until(&mut rig, thread, 5);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentQuiet))
    );
    assert!(failure_detail(&log).contains("for 5 min "));
    assert_eq!(retry_events(&log), [Some(CLAUDE_TITLE.to_owned()), None]);
    rig.finish();
}
