//! Helpers through the turn path (`turn/helpers.rs`), with Claude's frames put in as the ACP
//! reader hands them over (`AcpEvent::HelperUpdate`, `AcpEvent::HelperInBackground`), shaped as
//! the recorded run has them (`fake_agent`'s `claude-helper-background` scenario): one background
//! helper with two steps and words that hands back; a refused handback, then one that lands; two
//! foreground helpers interleaved, one failing; a Stop with two open; a turn that ends by itself
//! with a foreground and a background helper open; words before the launch input; a step
//! stamped with a helper never started; a replayed frame. The ends a run's
//! stop or a reopen brings are in `helpers_ends.rs`, the recorded runs in `helpers_recorded.rs`.

use std::collections::HashMap;

use clauseo_acp::{AcpEvent, AgentUpdate, MessageRole, Origin, ToolCall};
use clauseo_protocol::{
    AgentSessionId, CompactString, ContentBlock, DomainEvent, HelperId, HostCommand, StreamId,
    TextOrBlob, ThreadId, ToolCallContent, ToolCallId, ToolCallPatch, ToolCallStatus, ToolKind,
    TurnId,
};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::{AgentReply, TurnInput};
use crate::agent::AgentInput;

pub(super) const PROMPT: &str =
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#;
pub(super) const LEAD_STEP: &str = r#"{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"tc-0","title":"Read","kind":"read","status":"in_progress"}}}"#;
pub(super) const CANCEL: &str =
    r#"{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}"#;

/// The recorded launch input, its folder left out.
pub(super) const LAUNCH_INPUT: &str = r#"{"description":"Digest lease and notice","subagent_type":"paper-reader","prompt":"Read the files lease.txt and notice.txt in the working directory and return one short digest covering both documents."}"#;

/// The recorded handback's update.
pub(super) const DELIVERED: &str =
    r#"{"success":true,"message":"Report delivered to your caller."}"#;

/// One prompt with one lead step, answered `stop`.
pub(super) fn one_turn(stop: &str) -> String {
    format!(
        "{PROMPT}\n{LEAD_STEP}\n{{\"respond\":{{\"to\":\"p1\",\"result\":{{\"stopReason\":\"{stop}\"}}}}}}\n"
    )
}

pub(super) fn session() -> AgentSessionId {
    AgentSessionId::new("sess-1").unwrap()
}

pub(super) fn feed(rig: &mut Rig, thread: ThreadId, event: AcpEvent) {
    let input = Input::Agent {
        thread,
        input: AgentInput::Event(Box::new(event)),
    };
    rig.actor.on_input(input);
}

/// The lead's `update` in `turn`.
pub(super) fn lead(rig: &mut Rig, thread: ThreadId, turn: TurnId, update: AgentUpdate) {
    let event = AcpEvent::Update {
        session: session(),
        origin: Origin::Turn(turn),
        update,
    };
    feed(rig, thread, event);
}

/// `update` stamped with the helper the agent knows as `helper`, coming from `origin`.
pub(super) fn stamped(
    rig: &mut Rig,
    thread: ThreadId,
    origin: Origin,
    helper: &str,
    update: AgentUpdate,
) {
    let event = AcpEvent::HelperUpdate {
        session: session(),
        origin,
        helper: helper.into(),
        update,
    };
    feed(rig, thread, event);
}

pub(super) fn in_background(rig: &mut Rig, thread: ThreadId, turn: TurnId, helper: &str) {
    let event = AcpEvent::HelperInBackground {
        session: session(),
        origin: Origin::Turn(turn),
        helper: helper.into(),
    };
    feed(rig, thread, event);
}

/// A new step `acp_id` named `tool`, with `raw` as its input.
pub(super) fn call(acp_id: &str, tool: &str, raw: Option<&str>) -> AgentUpdate {
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
pub(super) fn patch(
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

pub(super) fn words(text: &str) -> AgentUpdate {
    AgentUpdate::MessageChunk {
        role: MessageRole::Answer,
        acp_message_id: None,
        content: ContentBlock::Text {
            text: text.to_owned(),
        },
    }
}

/// A chunk of the lead's answer, as its own message.
pub(super) fn answer(text: &str) -> AgentUpdate {
    AgentUpdate::MessageChunk {
        role: MessageRole::Answer,
        acp_message_id: Some("msg_lead".into()),
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

pub(super) const COMPLETED: Option<ToolCallStatus> = Some(ToolCallStatus::Completed);

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
pub(super) fn lead_step_logged(actor: &mut Actor, thread: ThreadId) -> bool {
    actor
        .loaded(StreamId::Thread(thread))
        .is_some_and(|loaded| loaded.sent.tool_call(&session(), "tc-0").is_some())
}

/// Sends a prompt and holds its answer once the lead's step is logged: the turn is live until
/// the test releases it.
pub(super) fn live_turn(rig: &mut Rig, thread: ThreadId) -> TurnId {
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
pub(super) fn story(log: &[DomainEvent]) -> Vec<String> {
    let mut names: HashMap<HelperId, String> = HashMap::new();
    let mut steps: HashMap<ToolCallId, String> = HashMap::new();
    let name = |names: &HashMap<HelperId, String>, id: Option<HelperId>| {
        id.map_or("-".to_owned(), |id| {
            names.get(&id).cloned().unwrap_or_else(|| "?".to_owned())
        })
    };
    let mut lines = Vec::new();
    for event in log {
        if let DomainEvent::ToolCallStarted(started) = event {
            steps.insert(started.tool_call, started.acp_id.to_string());
            let helper = name(&names, started.helper);
            lines.push(format!("step {} {helper}", started.acp_id));
        } else if let DomainEvent::HelperStarted(started) = event {
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
        } else if let DomainEvent::HelperText(text) = event {
            let helper = name(&names, Some(text.helper));
            lines.push(format!("words {helper}: {}", text.text));
        } else if let DomainEvent::HelperEnded(ended) = event {
            let helper = name(&names, Some(ended.helper));
            let summary = match &ended.summary {
                Some(TextOrBlob::Inline(text)) => text.lines().next().unwrap_or("").to_owned(),
                Some(TextOrBlob::Blob(_)) => "<blob>".to_owned(),
                None => "-".to_owned(),
            };
            lines.push(format!("end {helper} {:?} {summary}", ended.outcome));
        } else if let DomainEvent::TurnEnded(ended) = event {
            lines.push(format!("turn_ended {:?}", ended.stop));
        }
    }
    lines
}

/// The recorded shape: the `Agent` call, its input in an update, the background mark, its
/// launch note, two stamped steps with words between, the handback and its landed update. The
/// helper ends `Done` with its report; the handback is no step; the launch note is no summary;
/// a stamped thought is dropped.
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
        patch("toolu_A", Some(LAUNCH_INPUT), None, None),
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

/// A handback that says `success: false` sent nothing: the helper works on, and its next handback,
/// whose report comes in its update rather than its call, ends it with that report.
#[test]
fn a_refused_handback_keeps_the_helper_and_a_later_one_ends_it_with_its_report() {
    let mut rig = Rig::start("helpers-handback-twice", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(LAUNCH_INPUT)),
    );
    in_background(&mut rig, thread, turn, "toolu_A");
    let first = call(
        "toolu_H1",
        "SubagentHandback",
        Some(r#"{"message":"Draft."}"#),
    );
    stamped(&mut rig, thread, at, "toolu_A", first);
    let refused = r#"{"success":false,"message":"Nothing was sent."}"#;
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_H1", None, COMPLETED, Some(refused)),
    );
    stamped(
        &mut rig,
        thread,
        at,
        "toolu_A",
        call("toolu_R1", "Read", None),
    );
    stamped(
        &mut rig,
        thread,
        at,
        "toolu_A",
        call("toolu_H2", "SubagentHandback", None),
    );
    let report = r#"{"message":"Final digest."}"#;
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_H2", Some(report), None, None),
    );
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_H2", None, COMPLETED, Some(DELIVERED)),
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
            "end paper-reader Done Final digest.",
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
    let respond = r#"{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}"#;
    let steps = format!("{PROMPT}\n{LEAD_STEP}\n{CANCEL}\n{respond}\n");
    let mut rig = Rig::start("helpers-stop", &steps);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    rig.run_until("the lead's step", |actor| lead_step_logged(actor, thread));
    let at = Origin::Turn(turn);
    let input = r#"{"subagent_type":"paper-reader","prompt":"Read."}"#;
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(input)),
    );
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_B", "Agent", Some(input)),
    );
    in_background(&mut rig, thread, turn, "toolu_A");
    in_background(&mut rig, thread, turn, "toolu_B");
    stamped(&mut rig, thread, at, "toolu_B", words("Half"));
    rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step tc-0 -",
            "step toolu_A -",
            "start paper-reader call=toolu_A model=haiku label=- task=true",
            "step toolu_B -",
            "start paper-reader call=toolu_B model=haiku label=- task=true",
            "words paper-reader: Half",
            "end paper-reader Stopped -",
            "end paper-reader Stopped -",
            "turn_ended Cancelled",
        ]
    );
    rig.finish();
}

/// A turn that ends by itself ends a foreground helper that never handed back `Failed`, with no
/// summary, and leaves a background helper open: it works on after the prompt and ends later.
#[test]
fn a_turn_that_ends_by_itself_fails_a_foreground_helper_and_keeps_a_background_one() {
    let mut rig = Rig::start("helpers-open-at-end", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let input = r#"{"subagent_type":"paper-reader","prompt":"Read."}"#;
    let checker = r#"{"subagent_type":"checker","prompt":"Check."}"#;
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
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_B", "Agent", Some(checker)),
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
            "start checker call=toolu_B model=- label=- task=true",
            "end checker Failed -",
            "turn_ended EndTurn",
        ]
    );
    let open = rig
        .actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .map(clauseo_reducer::ThreadState::open_helpers);
    assert_eq!(open, Some(1), "the background helper is still open");
    rig.finish();
}

/// A helper that talks before its launch call's input has come: its words are kept under the
/// id the launch already took, which the helper then starts with.
#[test]
fn words_before_the_launch_input_stay_under_the_helper() {
    let mut rig = Rig::start("helpers-early-words", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    lead(&mut rig, thread, turn, call("toolu_A", "Agent", None));
    stamped(&mut rig, thread, at, "toolu_A", words("Early."));
    let input = r#"{"subagent_type":"paper-reader","prompt":"Read."}"#;
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_A", Some(input), None, None),
    );
    rig.release_held();
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    let started = log.iter().find_map(|event| {
        if let DomainEvent::HelperStarted(started) = event {
            Some(started.helper)
        } else {
            None
        }
    });
    let spoke = log.iter().find_map(|event| {
        if let DomainEvent::HelperText(text) = event {
            Some((text.helper, text.text.clone()))
        } else {
            None
        }
    });
    assert!(started.is_some(), "{log:?}");
    assert_eq!(spoke, started.map(|helper| (helper, "Early.".to_owned())));
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
