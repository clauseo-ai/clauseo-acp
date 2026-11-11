//! A turn the model refuses as too long to read ends `StopReason::TooLong` (decision 0057), on
//! the placeholder scenarios built from the adapters' source until the live recordings replace
//! them: Codex's full context and Claude's "Prompt is too long" (an AIR `sessionFailure` on
//! `end_turn`), and Codex's refusal of input over its 1,048,576 characters (an RPC error before
//! any turn starts). No `TurnFailed`, no limit stop, no limits read to wait for, the answer so
//! far and the question kept, and the queue held.

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

/// The answer so far in the two `sessionFailure` scenarios.
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
fn one_turn_ends_too_long(test: &str, id: &str, agent: AgentKind, scenario: &'static str) {
    let (dir, host, _reads) = start(test, id, agent, Scenario::Bundled(scenario));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("lease.txt", "x\n")], id);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, message) = host.prompt(thread, QUESTION);
    events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_too_long(&events, turn);
    assert_eq!(events.turn_text(turn), PARTIAL, "{}", events.dump());
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
    );
}

#[test]
fn claude_prompt_too_long_ends_too_long() {
    one_turn_ends_too_long(
        "claude_prompt_too_long_ends_too_long",
        CLAUDE,
        AgentKind::Claude,
        "claude-prompt-too-long",
    );
}

/// `codex-input-too-long` with one step added before its refusal: a permission request the test
/// answers once a second question is queued, so the queue is seen to hold. Everything else is
/// the bundled scenario's.
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
    assert_eq!(
        out.lines().count(),
        bundled.lines().count() + 2,
        "the gate went in"
    );
    out
}

/// Codex's refusal of an over-long input (an RPC error to `session/prompt`) ends the turn
/// `TooLong` at once, never the Codex limits read's 10 s wait nor `agent.protocol_error`; the
/// question stays, and the question queued behind it does not start.
#[test]
fn codex_input_too_long_ends_too_long_and_holds_the_queue() {
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
    assert_too_long(&events, first);
    let (_, asked) = user_message(&events, message);
    assert_eq!(
        asked.content,
        [ContentBlock::Text {
            text: QUESTION.to_owned()
        }]
    );
    assert!(held(&host, matter, thread), "the queue is not held");
    assert_eq!(starts(&events, second), 0, "{}", events.dump());
    assert!(events.turn_ended(second).is_none(), "{}", events.dump());
    host.shutdown();
}
