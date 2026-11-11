//! The host's check of Codex's own input cap (`codex_input.rs`, decision 0057): a `ChatGPT`
//! question Codex would refuse is refused with `prompt.too_long` before anything reaches the
//! agent, on every path that sends text to a Codex thread (start now, steer, send now, an edit of
//! a queued question); a question at exactly the cap is sent; Claude's questions are not checked.
//!
//! The question is checked again as it goes out, with the undo note the host adds then: a
//! question just under the cap, sent after an undo, is refused there for `ChatGPT` and sent for
//! Claude.
//!
//! The agent's scenario expects exactly the questions that may reach it, in order, so a refused
//! one that was sent anyway would be a mismatch.

use clauseo_host::codex_input::{CODEX_MAX_INPUT_CHARS, codex_input_chars};
use clauseo_protocol::{
    CommandOutcome, ContentBlock, DeliveryRequest, DomainEvent, ErrorCode, FileChoice,
    FileDecision, HostCommand, MatterPath, PromptBlock, StopReason, StreamId,
};
use serde_json::{Value, json};

use crate::codex::CODEX;
use crate::codex::turn_failed;
use crate::limits_host::CLAUDE;
use crate::permissions::{answer_from, until_first_request};
use crate::queue::{held, starts, until_queued};
use crate::support::{Dir, Scenario, matter_with_thread, send_prompt, text, wait};
use crate::write;

/// The bundled scenarios' folder.
const SCENARIOS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fake_agent/scenarios");
const NEXT: &str = "Then summarise the rent review.";

/// A permission request the test answers when it is done sending to the running turn.
fn gate() -> [Value; 2] {
    [
        json!({"request": {"id": "gate", "method": "session/request_permission", "params": {
            "sessionId": "sess-1", "toolCall": {"toolCallId": "call-gate", "title": "Read lease.txt",
            "kind": "read", "status": "pending"}, "options": [
            {"optionId": "allow_once", "name": "Yes, proceed", "kind": "allow_once"},
            {"optionId": "cancel", "name": "No", "kind": "reject_once"}]}}}),
        json!({"expect_response": {"id": "gate", "result": "{{any}}"}}),
    ]
}

fn prompt_of(label: &str, question: Option<&str>) -> Value {
    let params = match question {
        Some(question) => json!({"sessionId": "sess-1",
            "prompt": [{"type": "text", "text": question}]}),
        None => json!({"sessionId": "sess-1"}),
    };
    json!({"expect": {"method": "session/prompt", "as": label, "params": params}})
}

fn end_turn(label: &str) -> Value {
    json!({"respond": {"to": label, "result": {"stopReason": "end_turn"}}})
}

/// Writes scenario `name` on `handshake` with `steps` into `dir`, beside its `common/` copy.
pub(crate) fn written(dir: &Dir, name: &str, handshake: &str, steps: Vec<Value>) -> Scenario {
    let folder = dir.data().join("scenarios");
    let common = std::fs::read(format!("{SCENARIOS}/common/{handshake}")).unwrap();
    write::write(&folder.join("common").join(handshake), &common);
    let mut text = json!({"scenario": name, "description": "the Codex cap test's agent",
        "on_stdin_close": "exit"})
    .to_string();
    text.push('\n');
    for step in std::iter::once(json!({"include": handshake})).chain(steps) {
        text.push_str(&step.to_string());
        text.push('\n');
    }
    let file = folder.join(format!("{name}.jsonl"));
    write::write(&file, text.as_bytes());
    Scenario::Written(file)
}

fn prompt_with(
    thread: clauseo_protocol::ThreadId,
    question: &str,
    delivery: DeliveryRequest,
) -> HostCommand {
    HostCommand::SendPrompt {
        thread,
        content: text(question),
        delivery,
    }
}

/// One over the cap is refused on each path, before anything is sent; at the cap is sent.
#[test]
fn a_chatgpt_question_over_codex_s_cap_is_refused_on_every_path() {
    let at_cap = "a".repeat(CODEX_MAX_INPUT_CHARS);
    // One over in scalar values, though far under 6 MiB of escaped bytes.
    let over = "😀".repeat(CODEX_MAX_INPUT_CHARS + 1);
    let dir = Dir::new("codex_cap_every_path", &[]);
    let [request, answered] = gate();
    let steps = vec![
        prompt_of("p1", Some(&at_cap)),
        request,
        answered,
        end_turn("p1"),
        prompt_of("p2", Some(NEXT)),
        end_turn("p2"),
    ];
    let scenario = written(&dir, "codex-cap", "codex-handshake.jsonl", steps);
    dir.set_agents(&[(CODEX, scenario)]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("lease.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));

    // Start now: refused, nothing logged; the draft is the window's to keep.
    host.refused(send_prompt(thread, &over), ErrorCode::PromptTooLong);
    // At the cap: sent, and it runs until the test answers the agent's request.
    let (first, _) = host.prompt(thread, &at_cap);
    until_first_request(&mut events);
    // Steer and send now into the running turn: refused.
    for delivery in [DeliveryRequest::Steer, DeliveryRequest::SendNow] {
        host.refused(
            prompt_with(thread, &over, delivery),
            ErrorCode::PromptTooLong,
        );
    }
    // A queued question edited to one over the cap: refused, and the queued text stays.
    let (second, _) = host.prompt(thread, NEXT);
    until_queued(&mut events, second);
    let edit = HostCommand::EditQueuedPrompt {
        thread,
        turn: second,
        text: over,
    };
    host.refused(edit, ErrorCode::PromptTooLong);

    answer_from(&host, &mut events, thread, 0, &["allow_once"]);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    let messages = events.count(|event| matches!(event, DomainEvent::UserMessage(_)));
    assert_eq!(
        messages,
        2,
        "only the two questions sent: {}",
        events.dump()
    );
    let edits = events.count(|event| matches!(event, DomainEvent::QueuedPromptEdited(_)));
    assert_eq!(edits, 0, "{}", events.dump());
    host.shutdown();
}

/// Claude's questions are not held to Codex's cap: one over it is sent and answered.
#[test]
fn a_claude_question_over_codex_s_cap_is_sent() {
    let over = "a".repeat(CODEX_MAX_INPUT_CHARS + 1);
    let dir = Dir::new("codex_cap_not_claude", &[]);
    let steps = vec![prompt_of("p1", Some(&over)), end_turn("p1")];
    let scenario = written(&dir, "claude-cap", "claude-handshake.jsonl", steps);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("lease.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let outcome = host.ok(send_prompt(thread, &over));
    let CommandOutcome::PromptAccepted { turn, .. } = outcome else {
        panic!("SendPrompt answered {outcome:?}");
    };
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    host.shutdown();
}

/// With a file attached, codex-acp adds `[@name](uri)` to the turn's text and Codex counts it
/// (measured live: codex-boundary r3, r4, r4p). The question that, with its link, is exactly at
/// the cap is sent; one character more is refused.
#[test]
fn a_chatgpt_question_with_a_file_at_codex_s_cap_is_sent_and_one_over_is_refused() {
    let dir = Dir::new("codex_cap_with_a_link", &[]);
    let steps = vec![
        prompt_of("p1", None),
        end_turn("p1"),
        prompt_of("p2", None),
        end_turn("p2"),
    ];
    let scenario = written(&dir, "codex-cap-link", "codex-handshake.jsonl", steps);
    dir.set_agents(&[(CODEX, scenario)]);
    let host = dir.start(0);
    let (_, thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("lease draft.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let file = wait(
        "register_file",
        host.handle.register_file(folder.join("lease draft.txt")),
    )
    .unwrap();
    let with_file = |question: &str| HostCommand::SendPrompt {
        thread,
        content: vec![
            PromptBlock::Text {
                text: question.to_owned(),
            },
            PromptBlock::File { file },
        ],
        delivery: DeliveryRequest::Queue,
    };
    // A short question first, to read the link exactly as the host sends it.
    let CommandOutcome::PromptAccepted { turn, .. } = host.ok(with_file("Read it.")) else {
        panic!("not accepted");
    };
    events.until_turn_ended(turn);
    let link = events
        .events()
        .find_map(|event| match event {
            DomainEvent::UserMessage(message) => message.content.get(1).cloned(),
            _ => None,
        })
        .expect("the link as sent");
    let ContentBlock::ResourceLink { name, uri, .. } = &link else {
        panic!("{link:?}");
    };
    let cost = codex_input_chars(std::slice::from_ref(&link));
    assert_eq!(cost, 5 + name.chars().count() + uri.chars().count());
    let room = CODEX_MAX_INPUT_CHARS - cost;
    host.refused(with_file(&"a".repeat(room + 1)), ErrorCode::PromptTooLong);
    let CommandOutcome::PromptAccepted { turn, .. } = host.ok(with_file(&"a".repeat(room))) else {
        panic!("not accepted");
    };
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    host.shutdown();
}

/// A first turn that edits brief.txt, as either adapter may report it.
fn edits_the_brief() -> Vec<Value> {
    vec![
        prompt_of("p1", None),
        json!({"write_file": {"path": "brief.txt", "content": "Clause 4 final\n"}}),
        json!({"update": {"update": {"sessionUpdate": "tool_call", "toolCallId": "t1-edit-brief",
            "title": "Edit brief.txt", "kind": "edit", "status": "completed",
            "content": [{"type": "diff", "path": "{{cwd}}/brief.txt",
                "oldText": "Clause 4 draft\n", "newText": "Clause 4 final\n"}],
            "locations": [{"path": "{{cwd}}/brief.txt"}]}}}),
        end_turn("p1"),
    ]
}

/// Under Codex's cap alone; the undo note (well over ten characters) takes it over.
fn just_under() -> String {
    "a".repeat(CODEX_MAX_INPUT_CHARS - 10)
}

/// Runs the first turn on `agent`'s thread, undoes it so the next prompt carries the undo note,
/// then sends a question just under Codex's cap and queues a short one behind it, waiting for
/// that one's end too when `runs_on`. Returns the host's events, the two turns and whether the
/// queue shows held once they ended.
fn after_an_undo(
    test: &str,
    agent: &str,
    handshake: &str,
    then: Vec<Value>,
    runs_on: bool,
) -> (crate::observe::Events, [clauseo_protocol::TurnId; 2], bool) {
    let dir = Dir::new(test, &[]);
    let mut steps = edits_the_brief();
    steps.extend(then);
    let scenario = written(&dir, "undo-note-cap", handshake, steps);
    dir.set_agents(&[(agent, scenario)]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(
        &dir,
        &host,
        "m",
        &[("brief.txt", "Clause 4 draft\n")],
        agent,
    );
    let mut events = host.events(StreamId::Thread(thread));
    let (edited, _) = host.prompt(thread, "Finish the brief.");
    assert_eq!(events.until_turn_ended(edited).stop, StopReason::EndTurn);
    host.ok(HostCommand::UndoStep {
        thread,
        turn: edited,
        files: vec![FileDecision {
            path: MatterPath::new("brief.txt").unwrap(),
            choice: FileChoice::Undo,
        }],
    });
    events.until("StepUndone", |e| {
        e.position(|event| matches!(event, DomainEvent::StepUndone(_)))
            .is_some()
    });
    let (first, _) = host.prompt(thread, &just_under());
    let (second, _) = host.prompt(thread, NEXT);
    until_queued(&mut events, second);
    events.until_turn_ended(first);
    if runs_on {
        events.until_turn_ended(second);
    }
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    let held = held(&host, matter, thread);
    host.shutdown();
    (events, [first, second], held)
}

/// After an undo the host adds its note to the next prompt as it goes out. A `ChatGPT` question
/// just under Codex's cap goes over it with the note, so it is refused there: the turn fails
/// with `prompt.too_long`, nothing reaches the agent (its scenario expects no second prompt),
/// and the queue holds the question behind it.
#[test]
fn a_chatgpt_question_over_codex_s_cap_with_the_undo_note_is_refused_before_sending() {
    let (events, [first, second], held) = after_an_undo(
        "codex_cap_undo_note",
        CODEX,
        "codex-handshake.jsonl",
        Vec::new(),
        false,
    );
    assert_eq!(events.turn_ended(first).unwrap().stop, StopReason::Failed);
    let failed = turn_failed(&events, first).expect("the turn failed");
    assert_eq!(failed.code, ErrorCode::PromptTooLong, "{}", events.dump());
    assert!(held, "the queue held: {}", events.dump());
    assert_eq!(starts(&events, second), 0, "{}", events.dump());
}

/// The same question to Claude is sent, the note first (the agent expects exactly two text
/// blocks, the question second), and the queue moves on.
#[test]
fn a_claude_question_with_the_undo_note_over_codex_s_cap_is_sent() {
    let with_note = json!({"expect": {"method": "session/prompt", "as": "p2", "params": {
        "sessionId": "sess-1", "prompt": [{"type": "text", "text": "{{any}}"},
            {"type": "text", "text": just_under()}]}}});
    let then = vec![
        with_note,
        end_turn("p2"),
        prompt_of("p3", Some(NEXT)),
        end_turn("p3"),
    ];
    let (events, [first, second], _) = after_an_undo(
        "codex_cap_undo_note_claude",
        CLAUDE,
        "claude-handshake.jsonl",
        then,
        true,
    );
    assert_eq!(events.turn_ended(first).unwrap().stop, StopReason::EndTurn);
    assert_eq!(turn_failed(&events, first), None, "{}", events.dump());
    assert_eq!(
        events.turn_ended(second).map(|ended| ended.stop),
        Some(StopReason::EndTurn)
    );
}
