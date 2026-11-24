//! Helpers through the turn path (`turn/helpers.rs`), with Claude's frames put in as the ACP
//! reader hands them over (`AcpEvent::HelperUpdate`, `AcpEvent::HelperInBackground`), shaped as
//! the recorded run has them (issue 13, fixture S1): one background helper with two steps and
//! words that hands back; two foreground helpers interleaved, one failing; a Stop with two open;
//! a turn that ends with one still open; a step stamped with a helper never started; a replayed
//! frame; and the same view after the conversation is reopened.

use std::collections::HashMap;

use clauseo_acp::{AcpEvent, AgentUpdate, MessageRole, Origin, ToolCall};
use clauseo_protocol::{
    AgentSessionId, CompactString, ContentBlock, DomainEvent, HelperId, HostCommand, StreamId,
    TextOrBlob, ThreadId, ToolCallContent, ToolCallId, ToolCallPatch, ToolCallStatus, ToolKind,
    TurnId, ViewEntry,
};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::{AgentReply, TurnInput};
use crate::agent::AgentInput;

const PROMPT: &str =
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#;
const LEAD_STEP: &str = r#"{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"tc-0","title":"Read","kind":"read","status":"in_progress"}}}"#;
const CANCEL: &str = r#"{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}"#;

/// The launch input of S1.jsonl line 33, its folder moved under /Users/lawyer.
const S1_INPUT: &str = r#"{"description":"Digest lease and notice","subagent_type":"paper-reader","prompt":"Read the files lease.txt and notice.txt in the working directory /Users/lawyer/matter and return one short digest covering both documents."}"#;

/// The handback's update of S1.jsonl line 60.
const DELIVERED: &str = r#"{"success":true,"message":"Report delivered to your caller."}"#;

/// One prompt with one lead step, answered `stop`.
fn one_turn(stop: &str) -> String {
    format!(
        "{PROMPT}\n{LEAD_STEP}\n{{\"respond\":{{\"to\":\"p1\",\"result\":{{\"stopReason\":\"{stop}\"}}}}}}\n"
    )
}

fn session() -> AgentSessionId {
    AgentSessionId::new("sess-1").unwrap()
}

fn feed(rig: &mut Rig, thread: ThreadId, event: AcpEvent) {
    let input = Input::Agent {
        thread,
        input: AgentInput::Event(Box::new(event)),
    };
    rig.actor.on_input(input);
}

/// The lead's `update` in `turn`.
fn lead(rig: &mut Rig, thread: ThreadId, turn: TurnId, update: AgentUpdate) {
    let event = AcpEvent::Update {
        session: session(),
        origin: Origin::Turn(turn),
        update,
    };
    feed(rig, thread, event);
}

/// `update` stamped with the helper the agent knows as `helper`, coming from `origin`.
fn stamped(rig: &mut Rig, thread: ThreadId, origin: Origin, helper: &str, update: AgentUpdate) {
    let event = AcpEvent::HelperUpdate {
        session: session(),
        origin,
        helper: helper.into(),
        update,
    };
    feed(rig, thread, event);
}

fn in_background(rig: &mut Rig, thread: ThreadId, turn: TurnId, helper: &str) {
    let event = AcpEvent::HelperInBackground {
        session: session(),
        origin: Origin::Turn(turn),
        helper: helper.into(),
    };
    feed(rig, thread, event);
}

/// A new step `acp_id` named `tool`, with `raw` as its input.
fn call(acp_id: &str, tool: &str, raw: Option<&str>) -> AgentUpdate {
    AgentUpdate::ToolCall(ToolCall {
        acp_id: acp_id.into(),
        title: CompactString::from(tool),
        kind: ToolKind::Other,
        tool_name: Some(tool.into()),
        status: ToolCallStatus::Pending,
        locations: Vec::new(),
        raw_input: raw.map(|raw| TextOrBlob::Inline(raw.to_owned())),
        raw_output: None,
        content: Vec::new(),
    })
}

/// An update of step `acp_id`: its input, status and text, each when given.
fn patch(
    acp_id: &str,
    raw: Option<&str>,
    status: Option<ToolCallStatus>,
    text: Option<&str>,
) -> AgentUpdate {
    let content = text.map(|text| {
        vec![ToolCallContent::Content {
            content: ContentBlock::Text {
                text: text.to_owned(),
            },
        }]
    });
    AgentUpdate::ToolCallUpdate {
        acp_id: acp_id.into(),
        patch: ToolCallPatch {
            status,
            raw_input: raw.map(|raw| TextOrBlob::Inline(raw.to_owned())),
            content,
            ..ToolCallPatch::default()
        },
    }
}

fn words(text: &str) -> AgentUpdate {
    AgentUpdate::MessageChunk {
        role: MessageRole::Answer,
        acp_message_id: None,
        content: ContentBlock::Text {
            text: text.to_owned(),
        },
    }
}

fn thought(text: &str) -> AgentUpdate {
    AgentUpdate::MessageChunk {
        role: MessageRole::Thought,
        acp_message_id: None,
        content: ContentBlock::Text {
            text: text.to_owned(),
        },
    }
}

const COMPLETED: Option<ToolCallStatus> = Some(ToolCallStatus::Completed);

fn prompt_answer(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Turn(TurnInput::Agent {
            reply: AgentReply::Prompt { .. },
            ..
        })
    )
}

/// Whether the host has logged the lead's step `tc-0`.
fn lead_step_logged(actor: &mut Actor, thread: ThreadId) -> bool {
    actor
        .loaded(StreamId::Thread(thread))
        .is_some_and(|loaded| loaded.sent.tool_call(&session(), "tc-0").is_some())
}

/// Sends a prompt and holds its answer once the lead's step is logged: the turn is live until
/// the test releases it.
fn live_turn(rig: &mut Rig, thread: ThreadId) -> TurnId {
    let turn = send_prompt(rig, 10, thread);
    rig.run_holding("the lead's step", prompt_answer, |actor| {
        lead_step_logged(actor, thread)
    });
    if rig.held.is_empty() {
        rig.hold_next("the prompt's answer", prompt_answer);
    }
    turn
}

/// The log's helper events and steps after the turn's start, one line each, helpers by name and
/// steps by ACP id.
fn story(log: &[DomainEvent]) -> Vec<String> {
    let mut names: HashMap<HelperId, String> = HashMap::new();
    let mut steps: HashMap<ToolCallId, String> = HashMap::new();
    let name = |names: &HashMap<HelperId, String>, id: Option<HelperId>| {
        id.map_or("-".to_owned(), |id| {
            names.get(&id).cloned().unwrap_or_else(|| "?".to_owned())
        })
    };
    let mut lines = Vec::new();
    for event in log {
        match event {
            DomainEvent::ToolCallStarted(started) => {
                steps.insert(started.tool_call, started.acp_id.to_string());
                let helper = name(&names, started.helper);
                lines.push(format!("step {} {helper}", started.acp_id));
            }
            DomainEvent::HelperStarted(started) => {
                names.insert(started.helper, started.name.to_string());
                let call = started
                    .call
                    .and_then(|call| steps.get(&call).cloned())
                    .unwrap_or_default();
                let model = started.model.as_deref().unwrap_or("-");
                let label = started.label.as_deref().unwrap_or("-");
                let task = started.task.is_some();
                lines.push(format!(
                    "start {} call={call} model={model} label={label} task={task}",
                    started.name
                ));
            }
            DomainEvent::HelperText(text) => {
                let helper = name(&names, Some(text.helper));
                lines.push(format!("words {helper}: {}", text.text));
            }
            DomainEvent::HelperEnded(ended) => {
                let helper = name(&names, Some(ended.helper));
                let summary = match &ended.summary {
                    Some(TextOrBlob::Inline(text)) => text.lines().next().unwrap_or("").to_owned(),
                    Some(TextOrBlob::Blob(_)) => "<blob>".to_owned(),
                    None => "-".to_owned(),
                };
                lines.push(format!("end {helper} {:?} {summary}", ended.outcome));
            }
            DomainEvent::TurnEnded(ended) => lines.push(format!("turn_ended {:?}", ended.stop)),
            _ => {}
        }
    }
    lines
}

/// S1's shape: the `Agent` call, its input in an update, the background mark, its launch note,
/// two stamped steps with words between, the handback and its landed update. The helper ends
/// `Done` with its report; the handback is no step; the launch note is no summary; a stamped
/// thought is dropped.
#[test]
fn a_background_helper_shows_its_steps_and_words_and_ends_on_its_handback() {
    let mut rig = Rig::start("helpers-background", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    lead(&mut rig, thread, turn, call("toolu_A", "Agent", None));
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_A", Some(S1_INPUT), None, None),
    );
    in_background(&mut rig, thread, turn, "toolu_A");
    let note = "Async agent launched successfully.";
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_A", None, COMPLETED, Some(note)),
    );
    stamped(
        &mut rig,
        thread,
        at,
        "toolu_A",
        call("toolu_R1", "Read", None),
    );
    stamped(&mut rig, thread, at, "toolu_A", thought("Let me read."));
    stamped(&mut rig, thread, at, "toolu_A", words("The lease runs "));
    stamped(&mut rig, thread, at, "toolu_A", words("to 2031."));
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_R1", None, COMPLETED, None),
    );
    stamped(
        &mut rig,
        thread,
        at,
        "toolu_A",
        call("toolu_R2", "Grep", None),
    );
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_R2", None, COMPLETED, None),
    );
    let handback = call(
        "toolu_H",
        "SubagentHandback",
        Some(r#"{"message":"Digest."}"#),
    );
    stamped(&mut rig, thread, at, "toolu_A", handback);
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_H", None, COMPLETED, Some(DELIVERED)),
    );
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step tc-0 -",
            "step toolu_A -",
            "start paper-reader call=toolu_A model=haiku label=Digest lease and notice task=true",
            "step toolu_R1 paper-reader",
            "words paper-reader: The lease runs to 2031.",
            "step toolu_R2 paper-reader",
            "end paper-reader Done Digest.",
            "turn_ended EndTurn",
        ]
    );
    rig.finish();
}

/// The recorded frames themselves (fake_agent's `claude-helper-background` scenario, S1) through
/// the ACP reader: the helper starts from the `Agent` call's input, takes both stamped `Read`
/// steps and ends `Done` with its handback's digest; the handback is no step.
#[test]
fn the_recorded_background_helper_ends_on_its_handback() {
    let scenario =
        include_str!("../../../../../fake_agent/scenarios/claude-helper-background.jsonl");
    // The scenario's own header and handshake: the rig plays its own handshake.
    let steps: String = scenario
        .lines()
        .skip(2)
        .map(|line| format!("{line}\n"))
        .collect();
    let mut rig = Rig::start("helpers-recorded", &steps);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step toolu_01Gr3CDaJEng8pyzihUmv3Cq -",
            "start paper-reader call=toolu_01Gr3CDaJEng8pyzihUmv3Cq model=haiku \
             label=Digest lease and notice task=true",
            "step toolu_01WesqZa8jMTM4WMn2ELQNTp paper-reader",
            "step toolu_01VFDnvL4EbFFj1FnZthBh4g paper-reader",
            "end paper-reader Done DIGEST: lease.txt and notice.txt (matter-rec)",
            "turn_ended EndTurn",
        ]
    );
    rig.finish();
}

/// Two foreground helpers (no background mark) run at once: their steps and words interleave,
/// one ends with its `Agent` call completed (its text the summary), the other with it failed.
#[test]
fn two_foreground_helpers_interleave_and_end_with_their_calls() {
    let mut rig = Rig::start("helpers-two", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    let first = r#"{"subagent_type":"paper-reader","prompt":"Read the lease."}"#;
    let second = r#"{"name":"checker","prompt":"Check the notice.","model":"sonnet"}"#;
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(first)),
    );
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_B", "Task", Some(second)),
    );
    stamped(
        &mut rig,
        thread,
        at,
        "toolu_A",
        call("toolu_A1", "Read", None),
    );
    stamped(
        &mut rig,
        thread,
        at,
        "toolu_B",
        call("toolu_B1", "Read", None),
    );
    stamped(&mut rig, thread, at, "toolu_B", words("Checking."));
    stamped(&mut rig, thread, at, "toolu_A", words("Reading."));
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_A1", None, COMPLETED, None),
    );
    let failed = Some(ToolCallStatus::Failed);
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_B", None, failed, Some("It broke.")),
    );
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_A", None, COMPLETED, Some("Term to 2031.")),
    );
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step tc-0 -",
            "step toolu_A -",
            "start paper-reader call=toolu_A model=haiku label=- task=true",
            "step toolu_B -",
            "start checker call=toolu_B model=sonnet label=- task=true",
            "step toolu_A1 paper-reader",
            "step toolu_B1 checker",
            "words checker: Checking.",
            "words paper-reader: Reading.",
            "end checker Failed -",
            "end paper-reader Done Term to 2031.",
            "turn_ended EndTurn",
        ]
    );
    rig.finish();
}

/// The lawyer's Stop ends both open helpers `Stopped` in the cancel's commit, before the turn
/// ends.
#[test]
fn a_stop_ends_every_open_helper_stopped() {
    let mut rig = Rig::start("helpers-open-at-end", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let input = r#"{"subagent_type":"paper-reader","prompt":"Read."}"#;
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(input)),
    );
    in_background(&mut rig, thread, turn, "toolu_A");
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_A", None, COMPLETED, Some("Launched.")),
    );
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step tc-0 -",
            "step toolu_A -",
            "start paper-reader call=toolu_A model=haiku label=- task=true",
            "end paper-reader Failed -",
            "turn_ended EndTurn",
        ]
    );
    rig.finish();
}

/// A step stamped with a helper the host never started shows as the lead's; a replayed stamped
/// frame and a late one are dropped.
#[test]
fn an_unknown_stamp_shows_flat_and_a_replayed_frame_is_dropped() {
    let mut rig = Rig::start("helpers-unknown", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    stamped(
        &mut rig,
        thread,
        at,
        "toolu_X",
        call("toolu_X1", "Read", None),
    );
    stamped(&mut rig, thread, at, "toolu_X", words("Lost words."));
    stamped(
        &mut rig,
        thread,
        Origin::Replay,
        "toolu_X",
        call("toolu_X2", "Read", None),
    );
    stamped(
        &mut rig,
        thread,
        Origin::Late(turn),
        "toolu_X",
        call("toolu_X3", "Read", None),
    );
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        ["step tc-0 -", "step toolu_X1 -", "turn_ended EndTurn"]
    );
    rig.finish();
}

/// The helper's view after a reopen is the view before it: the same `Helper` and `HelperText`
/// entries, and its steps still name it.
#[test]
fn a_reopened_conversation_shows_the_same_helper() {
    let mut rig = Rig::start("helpers-reopen", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(S1_INPUT)),
    );
    in_background(&mut rig, thread, turn, "toolu_A");
    stamped(
        &mut rig,
        thread,
        at,
        "toolu_A",
        call("toolu_R1", "Read", None),
    );
    stamped(&mut rig, thread, at, "toolu_A", words("Read it."));
    let handback = call(
        "toolu_H",
        "SubagentHandback",
        Some(r#"{"message":"Digest."}"#),
    );
    stamped(&mut rig, thread, at, "toolu_A", handback);
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_H", None, COMPLETED, Some(DELIVERED)),
    );
    rig.release_held();
    until_ended(&mut rig, thread);
    let before = helper_entries(&mut rig, thread);
    assert_eq!(before.len(), 3, "{before:?}");
    let mut rig = rig.restart(2);
    let after = helper_entries(&mut rig, thread);
    assert_eq!(after, before);
    rig.finish();
}

/// The thread's view entries that belong to a helper: its card, its words, its steps.
fn helper_entries(rig: &mut Rig, thread: ThreadId) -> Vec<ViewEntry> {
    let loaded = rig.actor.loaded(StreamId::Thread(thread)).unwrap();
    let state = loaded.state.as_thread().unwrap();
    state
        .entries()
        .values()
        .filter(|entry| match entry {
            ViewEntry::Helper(_) | ViewEntry::HelperText(_) => true,
            ViewEntry::ToolCall(call) => call.helper.is_some(),
            _ => false,
        })
        .cloned()
        .collect()
}
