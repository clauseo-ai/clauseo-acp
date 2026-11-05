//! Claude's `AskUserQuestion`, as claude-agent-acp 0.89.1 sends it: a form elicitation whose
//! schema holds each question's options (`oneOf` for one pick, `items.anyOf` for several) and
//! an "Other" field per question. The options reach the view whole, the lawyer's answer goes
//! back as the form's values and the turn goes on; a Stop ends the turn and the next runs.

use clauseo_protocol::{
    CommandOutcome, DomainEvent, ElicitationEntry, ElicitationId, ElicitationRequest,
    ElicitationResponse, ErrorCode, HostCommand, RawPayload, StopReason, StreamId, ViewEntry,
    ViewPatchBody,
};
use serde_json::{Value, json};

use crate::observe::{Events, View};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread};

/// The `elicitation/create` params of `scenario`'s request `ask-1`, read from its file.
fn ask_params(scenario: &str) -> Value {
    let path = clauseo_fake_agent::scenario_path(scenario);
    let text = std::fs::read_to_string(&path).unwrap();
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|step| {
            let request = step.get("request")?;
            (request.get("id")? == "ask-1").then(|| request["params"].clone())
        })
        .unwrap_or_else(|| panic!("{} has no request ask-1", path.display()))
}

/// The elicitation the thread logged.
fn requested(events: &mut Events) -> ElicitationId {
    events.until("ElicitationRequested", |e| {
        e.position(|event| matches!(event, DomainEvent::ElicitationRequested(_)))
            .is_some()
    });
    events
        .events()
        .find_map(|event| match event {
            DomainEvent::ElicitationRequested(requested) => Some(requested.elicitation),
            _ => None,
        })
        .unwrap()
}

/// The last elicitation entry the view sent.
fn entry(view: &View) -> Option<ElicitationEntry> {
    view.patches()
        .filter_map(|patch| match &patch.body {
            ViewPatchBody::Upsert { entry } => match &**entry {
                ViewEntry::Elicitation(entry) => Some(entry.clone()),
                _ => None,
            },
            _ => None,
        })
        .last()
}

fn values(json: Value) -> RawPayload {
    serde_json::from_value(json).unwrap()
}

#[test]
fn ask_user_question_options_reach_the_view_and_an_answer_resumes_the_turn() {
    let dir = Dir::new(
        "ask_user_question_answer",
        &[(FAKE, Scenario::Bundled("ask-user-question"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let stream = StreamId::Thread(thread);
    let mut view = host.view(stream, None);
    let mut events = host.events(stream);
    let (turn, _) = host.prompt(thread, "Draft the services agreement.");
    let elicitation = requested(&mut events);

    let shown = view
        .until("the elicitation entry", |v| entry(v).is_some())
        .drain_ready();
    let shown = entry(shown).unwrap();
    assert_eq!(shown.elicitation, elicitation);
    let ElicitationRequest::Form { message, schema } = &shown.request else {
        panic!("not a form: {:?}", shown.request);
    };
    let sent = ask_params("ask-user-question");
    assert_eq!(message, sent["message"].as_str().unwrap());
    // Every option, with its description and `_meta` preview, and every Other field.
    assert_eq!(
        serde_json::to_value(schema).unwrap(),
        sent["requestedSchema"]
    );

    // A pick of the wrong type and a field the form lacks are refused; the agent still waits.
    host.refused(
        HostCommand::AnswerElicitation {
            thread,
            elicitation,
            response: ElicitationResponse::Accept {
                values: values(json!({"question_0": ["New York"]})),
            },
        },
        ErrorCode::ElicitationInvalidValues,
    );
    host.refused(
        HostCommand::AnswerElicitation {
            thread,
            elicitation,
            response: ElicitationResponse::Accept {
                values: values(json!({"question_9": "Fees"})),
            },
        },
        ErrorCode::ElicitationInvalidValues,
    );

    let answer = json!({
        "question_0": "New York",
        "question_1": ["Fees"],
        "question_1_custom": "Exhibits",
    });
    assert_eq!(
        host.ok(HostCommand::AnswerElicitation {
            thread,
            elicitation,
            response: ElicitationResponse::Accept {
                values: values(answer),
            },
        }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_text(turn),
        "New York law, with the fees schedule and exhibits."
    );
    host.shutdown();
}

#[test]
fn a_stop_on_ask_user_question_ends_the_turn_and_the_next_one_runs() {
    let dir = Dir::new(
        "ask_user_question_stop",
        &[(FAKE, Scenario::Bundled("ask-user-question-stop"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Draft the services agreement.");
    requested(&mut events);
    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);

    let (second, _) = host.prompt(thread, "Are you there?");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    events.assert_no_mismatch();
    host.shutdown();
}
