//! A turn the model refuses as too long to read ends `StopReason::TooLong` (decision 0057):
//! Claude's "Prompt is too long" (recorded live) and Codex's full context (from codex-acp's
//! source), each an AIR `sessionFailure` on `end_turn`. No `TurnFailed`, no limit stop, no limits
//! read to wait for, the answer so far and the question kept. Codex's recorded refusal of an
//! over-long input says only "Codex encountered an internal error.", which is not read as too long;
//! what the host does with it today is pinned here too.

use std::sync::Arc;

use clauseo_protocol::{AgentKind, ContentBlock, DomainEvent, StopReason, StreamId, TurnId};
use serde_json::json;

use crate::codex::{CODEX, turn_failed};
use crate::limits_host::{CLAUDE, Feed, Reads, usage};
use crate::observe::Events;
use crate::permissions::{answer_from, until_first_request};
use crate::queue::{held, settle, starts, until_queued, user_message};
use crate::support::{Dir, Running, Scenario, matter_with_thread};
use crate::write;

/// The bundled scenarios' folder.
const SCENARIOS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fake_agent/scenarios");

/// The answer so far in `codex-context-full`; the recorded Claude refusal has none.
const PARTIAL: &str = "Reading the lease. ";
const QUESTION: &str = "Read the whole lease and list every break clause.";

/// A host whose `id` agent plays `scenario`, reading limits through scripted reads that answer
/// the start-up read only, so a turn that waited for a limits read would never end.
fn start(test: &str, id: &str, agent: AgentKind, scenario: Scenario) -> (Dir, Running, Reads) {
    let dir = Dir::new(test, &[(id, scenario)]);
    let reads = Reads::new();
    let reader = Arc::new(reads.clone());
    let host = dir.start_with(0, move |config| config.with_limits_reader(reader));
    let mut feed = Feed::new(&host);
    feed.view("the current view");
    reads.answer(agent, usage(agent, 10.0, None));
    feed.changed("the start-up read");
    drop(feed);
    (dir, host, reads)
}

/// `turn` ended `TooLong` with nothing else: no `TurnFailed`, no limit stop.
fn assert_too_long(events: &Events, turn: TurnId) {
    let ended = events.turn_ended(turn).expect("the turn ended");
    assert_eq!(ended.stop, StopReason::TooLong, "{}", events.dump());
    assert_eq!(turn_failed(events, turn), None, "{}", events.dump());
    assert!(
        events
            .position(|e| matches!(e, DomainEvent::TurnStoppedAtLimit(_)))
            .is_none(),
        "{}",
        events.dump()
    );
}

/// One turn of `scenario` ends `TooLong`, keeping the answer so far and the question.
fn one_turn_ends_too_long(
    test: &str,
    id: &str,
    agent: AgentKind,
    scenario: &'static str,
    partial: &str,
) {
    let (dir, host, _reads) = start(test, id, agent, Scenario::Bundled(scenario));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("lease.txt", "x\n")], id);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, message) = host.prompt(thread, QUESTION);
    events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_too_long(&events, turn);
    assert_eq!(events.turn_text(turn), partial, "{}", events.dump());
    let (_, asked) = user_message(&events, message);
    assert_eq!(
        asked.content,
        [ContentBlock::Text {
            text: QUESTION.to_owned()
        }]
    );
    host.shutdown();
}

#[test]
fn codex_context_full_ends_too_long() {
    one_turn_ends_too_long(
        "codex_context_full_ends_too_long",
        CODEX,
        AgentKind::Codex,
        "codex-context-full",
        PARTIAL,
    );
}

#[test]
fn claude_prompt_too_long_ends_too_long() {
    one_turn_ends_too_long(
        "claude_prompt_too_long_ends_too_long",
        CLAUDE,
        AgentKind::Claude,
        "claude-prompt-too-long",
        "",
    );
}

/// The recorded `codex-input-too-long` with a permission request added before its refusal, which
/// the test answers once a second question is queued, and an answer to that second question.
/// Everything else is the bundled scenario's.
fn gated_input_too_long() -> String {
    let bundled =
        std::fs::read_to_string(format!("{SCENARIOS}/codex-input-too-long.jsonl")).unwrap();
    let gate = [
        json!({"request": {"id": "gate", "method": "session/request_permission", "params": {
            "sessionId": "sess-1", "toolCall": {"toolCallId": "call-gate", "title": "Read lease.txt",
            "kind": "read", "status": "pending"}, "options": [
            {"optionId": "allow_once", "name": "Yes, proceed", "kind": "allow_once"},
            {"optionId": "cancel", "name": "No", "kind": "reject_once"}]}}}),
        json!({"expect_response": {"id": "gate", "result": "{{any}}"}}),
    ];
    let second = [
        json!({"expect": {"method": "session/prompt", "as": "p2", "params": {"sessionId": "sess-1"}}}),
        json!({"respond": {"to": "p2", "result": {"stopReason": "end_turn"}}}),
    ];
    let mut out = String::new();
    for line in bundled.lines() {
        if line.starts_with(r#"{"respond":{"to":"p1""#) {
            for step in &gate {
                out.push_str(&step.to_string());
                out.push('\n');
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    for step in &second {
        out.push_str(&step.to_string());
        out.push('\n');
    }
    assert_eq!(
        out.lines().count(),
        bundled.lines().count() + 4,
        "the gate and the second answer went in"
    );
    out
}

/// TODAY's behaviour with Codex's recorded refusal of a question over its 1,048,576 characters
/// (decision 0057 leaves it open): the wire says only "Codex encountered an internal error."
/// (category `service`), so the turn ends `EndTurn` with no answer text and no `TurnFailed`; the
/// failure is only logged as a warning, and the queue moves on to the next question. The lawyer is
/// told nothing. The host's own Codex check before sending is what keeps such a question back.
#[test]
fn codex_input_refusal_today_ends_end_turn_and_moves_the_queue_on() {
    let dir = Dir::new("codex_input_too_long_holds_the_queue", &[]);
    // A scenario read from a path takes its includes from the `common/` folder beside it, and
    // its header names its file.
    let gated = dir.data().join("gated");
    let handshake = std::fs::read(format!("{SCENARIOS}/common/codex-handshake.jsonl")).unwrap();
    write::write(&gated.join("common/codex-handshake.jsonl"), &handshake);
    let file = gated.join("codex-input-too-long.jsonl");
    write::write(&file, gated_input_too_long().as_bytes());
    dir.set_agents(&[(CODEX, Scenario::Written(file))]);
    let reads = Reads::new();
    let reader = Arc::new(reads.clone());
    let host = dir.start_with(0, move |config| config.with_limits_reader(reader));
    let mut feed = Feed::new(&host);
    feed.view("the current view");
    reads.answer(AgentKind::Codex, usage(AgentKind::Codex, 10.0, None));
    feed.changed("the start-up read");
    drop(feed);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("lease.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, message) = host.prompt(thread, QUESTION);
    until_first_request(&mut events);
    let (second, _) = host.prompt(thread, "Then summarise the rent review.");
    until_queued(&mut events, second);
    answer_from(&host, &mut events, thread, 0, &["allow_once"]);
    events.until_turn_ended(first);
    settle(&host, &mut events);
    events.assert_no_mismatch();
    let ended = events.turn_ended(first).expect("the turn ended");
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(turn_failed(&events, first), None, "{}", events.dump());
    assert_eq!(events.turn_text(first), "", "{}", events.dump());
    let (_, asked) = user_message(&events, message);
    assert_eq!(
        asked.content,
        [ContentBlock::Text {
            text: QUESTION.to_owned()
        }]
    );
    let next = events.until_turn_ended(second);
    assert_eq!(next.stop, StopReason::EndTurn, "{}", events.dump());
    assert!(!held(&host, matter, thread), "the queue held");
    assert_eq!(starts(&events, second), 1, "{}", events.dump());
    host.shutdown();
}

/// One turn of `agent` whose agent sends `failure` as an AIR `sessionFailure`, on a
/// `session_info_update` and, when `on_answer`, on its `end_turn` too; returns how it ended.
fn ends_after_failure(
    test: &str,
    agent: &str,
    failure: &serde_json::Value,
    on_answer: bool,
) -> StopReason {
    let dir = Dir::new(test, &[]);
    let meta = json!({"jetbrains": {"air": {"version": 1, "sessionFailure": failure}}});
    let result = if on_answer {
        json!({"stopReason": "end_turn", "_meta": meta})
    } else {
        json!({"stopReason": "end_turn"})
    };
    let handshake = if agent == CODEX {
        "codex-handshake.jsonl"
    } else {
        "claude-handshake.jsonl"
    };
    let steps = vec![
        json!({"expect": {"method": "session/prompt", "as": "p1", "params": {"sessionId": "sess-1"}}}),
        json!({"update": {"update": {"sessionUpdate": "session_info_update", "_meta": meta}}}),
        json!({"respond": {"to": "p1", "result": result}}),
    ];
    let scenario = crate::codex_cap::written(&dir, "failure", handshake, steps);
    dir.set_agents(&[(agent, scenario)]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("lease.txt", "x\n")], agent);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, QUESTION);
    let stop = events.until_turn_ended(turn).stop;
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(turn_failed(&events, turn), None, "{}", events.dump());
    host.shutdown();
    stop
}

fn context_exhausted() -> serde_json::Value {
    json!({"id": "t1:error", "revision": 1, "category": "limit", "severity": "error",
        "title": "This Claude turn reached its configured limit.", "actions": ["new_session"]})
}

/// claude-agent-acp sends `context_exhausted` (category `limit`, actions `["new_session"]`) for
/// its configured turn or output limit: not "too long" for Claude, so the turn ends as the agent
/// said; the same shape from codex-acp, its full context, ends `TooLong`.
#[test]
fn context_exhausted_is_too_long_only_for_codex() {
    let claude = ends_after_failure(
        "context_exhausted_claude",
        CLAUDE,
        &context_exhausted(),
        true,
    );
    assert_eq!(claude, StopReason::EndTurn);
    let codex = ends_after_failure("context_exhausted_codex", CODEX, &context_exhausted(), true);
    assert_eq!(codex, StopReason::TooLong);
}

/// A too-long failure sent only on the `session_info_update`, the answer a plain `end_turn`,
/// still ends the turn `TooLong`.
#[test]
fn a_too_long_failure_only_on_the_update_ends_too_long() {
    let failure = json!({"id": "t1:error", "revision": 1, "category": "service",
        "severity": "error", "title": "Prompt is too long", "actions": ["retry"]});
    let stop = ends_after_failure("too_long_update_only", CLAUDE, &failure, false);
    assert_eq!(stop, StopReason::TooLong);
}
