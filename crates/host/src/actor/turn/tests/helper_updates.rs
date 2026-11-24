//! A helper's events (`AcpEvent::HelperUpdate`, `AcpEvent::HelperInBackground`) until the host's
//! helper cards land: a helper's tool call is logged exactly as the same call from the lead, and
//! the helper in the background adds nothing (`updates.rs`, `on_event`). The events are put in
//! by hand, during a live turn whose answer the test holds back.

use std::mem::Discriminant;

use clauseo_acp::{AcpEvent, AgentUpdate, Origin, ToolCall};
use clauseo_protocol::{
    AgentSessionId, CompactString, DomainEvent, ToolCallPatch, ToolCallStatus, ToolKind,
};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::{AgentReply, TurnInput};
use crate::agent::AgentInput;

const SCRIPT: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

/// The lead's `Agent` call that launched the helper.
const LAUNCH: &str = "toolu_01Launch";

/// The helper's step.
const STEP: &str = "toolu_02Read";

fn prompt_answer(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Turn(TurnInput::Agent {
            reply: AgentReply::Prompt { .. },
            ..
        })
    )
}

/// The step's start, pending.
fn step_call() -> AgentUpdate {
    AgentUpdate::ToolCall(ToolCall {
        acp_id: CompactString::from(STEP),
        title: CompactString::from("Read lease.txt"),
        kind: ToolKind::Read,
        tool_name: Some(CompactString::from("Read")),
        status: ToolCallStatus::Pending,
        locations: Vec::new(),
        raw_input: None,
        raw_output: None,
        content: Vec::new(),
    })
}

/// The step's end.
fn step_done() -> AgentUpdate {
    AgentUpdate::ToolCallUpdate {
        acp_id: CompactString::from(STEP),
        patch: ToolCallPatch {
            status: Some(ToolCallStatus::Completed),
            ..ToolCallPatch::default()
        },
    }
}

/// What a log says, for comparing two runs: each event's kind, and each tool call's ACP id,
/// title, tool name and statuses.
type Seen = (
    Vec<Discriminant<DomainEvent>>,
    Vec<(String, String, Option<String>, Vec<ToolCallStatus>)>,
);

/// Plays one turn of [`SCRIPT`] and puts `events` (each given the turn's origin) in while the
/// turn is live; returns what the thread's log then says.
fn logged(name: &str, events: impl Fn(Origin) -> Vec<AcpEvent>) -> Seen {
    let mut rig = Rig::start(name, SCRIPT);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    rig.hold_next("the prompt's answer", prompt_answer);
    for event in events(Origin::Turn(turn)) {
        let input = Input::Agent {
            thread,
            input: AgentInput::Event(Box::new(event)),
        };
        rig.actor.on_input(input);
    }
    rig.release_held();
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    rig.finish();
    let kinds = log.iter().map(std::mem::discriminant).collect();
    let mut calls = Vec::new();
    for event in &log {
        match event {
            DomainEvent::ToolCallStarted(started) => calls.push((
                started.acp_id.to_string(),
                started.title.to_string(),
                started.tool_name.as_ref().map(ToString::to_string),
                vec![started.status],
            )),
            DomainEvent::ToolCallUpdated(updated) => {
                if let (Some(call), Some(status)) = (calls.last_mut(), updated.patch.status) {
                    call.3.push(status);
                }
            }
            _ => {}
        }
    }
    (kinds, calls)
}

fn session() -> AgentSessionId {
    AgentSessionId::new("sess-1").unwrap()
}

#[test]
fn a_helper_tool_call_is_logged_as_the_lead_one_is() {
    let lead = logged("helper-as-lead", |origin| {
        vec![
            AcpEvent::Update {
                session: session(),
                origin,
                update: step_call(),
            },
            AcpEvent::Update {
                session: session(),
                origin,
                update: step_done(),
            },
        ]
    });
    let helper = logged("helper-tagged", |origin| {
        vec![
            AcpEvent::HelperInBackground {
                session: session(),
                origin,
                helper: CompactString::from(LAUNCH),
            },
            AcpEvent::HelperUpdate {
                session: session(),
                origin,
                helper: CompactString::from(LAUNCH),
                update: step_call(),
            },
            AcpEvent::HelperUpdate {
                session: session(),
                origin,
                helper: CompactString::from(LAUNCH),
                update: step_done(),
            },
        ]
    });
    let step = (
        STEP.to_owned(),
        "Read lease.txt".to_owned(),
        Some("Read".to_owned()),
        vec![ToolCallStatus::Pending, ToolCallStatus::Completed],
    );
    assert_eq!(lead.1, [step]);
    assert_eq!(helper, lead);
}
