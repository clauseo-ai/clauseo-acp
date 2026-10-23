//! One example of every view entry kind, built from the fixtures' ids and the event examples.
//!
//! A test file that uses it includes it next to `fixtures` and `examples`:
//!
//! ```text
//! #[cfg(test)]
//! #[path = "support/entries.rs"]
//! pub mod entries;
//! ```

use clauseo_protocol::{
    AgentContentEntry, AgentTextEntry, CheckpointKind, ContentBlock, ContextUsage, Deliverable,
    Delivery, ElicitationEntry, ElicitationRequest, ElicitationState, ExpiryReason, HeaderEntry,
    ImportSummary, LimitStopEntry, LimitStopReason, MarkerEntry, MarkerKind, MatterHeaderEntry,
    MatterKind, NoticeEntry, NoticeLevel, PermissionChoice, PermissionEntry, PermissionKind,
    PermissionOption, PermissionState, Principal, StepEntry, StopReason, TextKind, TextOrBlob,
    ThreadStatus, Timestamp, ToolCallEntry, UserMessageEntry, UserMessageStatus, VersionEntry,
    ViewEntry,
};

use crate::examples::{file_changes, tool_call_started};
use crate::fixtures::{
    agent, checkpoint_id, count, elicitation_id, event_id, matter_id, message_id, path,
    permission_request_id, raw, seq, short, thread_id, turn_id,
};

/// One example of every `ViewEntry` kind, in declaration order.
pub fn every_entry() -> Vec<ViewEntry> {
    [
        thread_entries(),
        approval_entries(),
        turn_entries(),
        matter_entries(),
    ]
    .concat()
}

/// A thread's header, with its usage: the one `HeaderEntry` the tests build.
pub fn header_entry() -> HeaderEntry {
    HeaderEntry {
        order: seq(1),
        thread: thread_id(2),
        matter: matter_id(1),
        title: Some(short("Draft SPA")),
        agent: agent("claude-acp"),
        status: ThreadStatus::Idle,
        queue_held: true,
        mode_id: Some(short("plan")),
        config_options: vec![],
        commands: vec![],
        plan: vec![],
        usage: Some(ContextUsage {
            context_used: Some(count(1200)),
            context_size: Some(count(200_000)),
            cost: None,
        }),
    }
}

/// An answer's text: the one `AgentTextEntry` the tests build.
pub fn agent_text_entry() -> AgentTextEntry {
    AgentTextEntry {
        order: seq(3),
        message: message_id(9),
        turn: turn_id(3),
        kind: TextKind::Answer,
        text: "The lease ends in 2030.".to_owned(),
    }
}

/// The header and the transcript's messages and tool calls.
fn thread_entries() -> Vec<ViewEntry> {
    let tool = tool_call_started();
    vec![
        ViewEntry::Header(header_entry()),
        ViewEntry::UserMessage(UserMessageEntry {
            order: seq(2),
            message: message_id(5),
            turn: turn_id(3),
            content: vec![ContentBlock::Text {
                text: "Summarise".to_owned(),
            }],
            delivery: Delivery::Queue,
            status: UserMessageStatus::Queued,
            edited: false,
            steer: None,
        }),
        ViewEntry::AgentText(agent_text_entry()),
        ViewEntry::AgentContent(AgentContentEntry {
            order: seq(4),
            event: event_id(20),
            turn: turn_id(3),
            message: message_id(9),
            kind: TextKind::Thought,
            block: ContentBlock::Image {
                data: TextOrBlob::Inline("iVBORw0KGgo=".to_owned()),
                mime_type: short("image/png"),
                uri: None,
            },
        }),
        ViewEntry::ToolCall(Box::new(ToolCallEntry {
            order: seq(5),
            tool_call: tool.tool_call,
            turn: tool.turn,
            title: tool.title,
            kind: tool.kind,
            tool_name: tool.tool_name,
            status: tool.status,
            locations: tool.locations,
            raw_input: tool.raw_input,
            raw_output: tool.raw_output,
            content: tool.content,
        })),
    ]
}

/// A permission request and an elicitation.
fn approval_entries() -> Vec<ViewEntry> {
    vec![
        ViewEntry::Permission(PermissionEntry {
            order: seq(6),
            request: permission_request_id(10),
            tool_call: None,
            options: vec![PermissionOption {
                option_id: short("allow"),
                kind: PermissionKind::AllowOnce,
                agent_label: "Yes".to_owned(),
            }],
            deliverable: Deliverable::Live,
            state: PermissionState::Resolved {
                choice: PermissionChoice::Selected {
                    option_id: short("allow"),
                    kind: PermissionKind::AllowOnce,
                },
                by: Principal::LocalDesktop,
            },
        }),
        ViewEntry::Elicitation(ElicitationEntry {
            order: seq(7),
            elicitation: elicitation_id(12),
            request: ElicitationRequest::Form {
                message: "Which party?".to_owned(),
                schema: raw(r#"{"type":"object"}"#),
            },
            state: ElicitationState::Expired {
                reason: ExpiryReason::TurnEnded,
            },
        }),
    ]
}

/// The step card, the at-limit card, a notice and a marker.
fn turn_entries() -> Vec<ViewEntry> {
    vec![
        ViewEntry::Step(StepEntry {
            order: seq(8),
            turn: turn_id(3),
            stop: StopReason::EndTurn,
            failure: None,
            files: file_changes(),
            skipped: vec![],
            checkpoint_after: Some(checkpoint_id(14)),
            undone: false,
            usage: None,
        }),
        ViewEntry::LimitStop(LimitStopEntry {
            order: seq(9),
            turn: turn_id(6),
            window: Some("five_hour".to_owned()),
            resets_at: Some(Timestamp::new(1_760_000_000_000).unwrap()),
            reason: LimitStopReason::PlanWindow,
        }),
        ViewEntry::Notice(NoticeEntry {
            order: seq(10),
            event: event_id(21),
            level: NoticeLevel::Warning,
            text: "Context nearly full".to_owned(),
        }),
        ViewEntry::Marker(MarkerEntry {
            order: seq(11),
            event: event_id(22),
            kind: MarkerKind::StepUndone {
                turn: turn_id(3),
                checkpoint: checkpoint_id(15),
                restored: vec![path("a.docx")],
                kept: vec![],
            },
        }),
    ]
}

/// A matter's header and one version.
fn matter_entries() -> Vec<ViewEntry> {
    vec![
        ViewEntry::MatterHeader(MatterHeaderEntry {
            order: seq(1),
            matter: matter_id(1),
            name: "Smith v Jones".to_owned(),
            kind: MatterKind::Folder,
            import: Some(ImportSummary {
                files: 12,
                bytes: count(4_000_000),
                cloud_only: 1,
            }),
        }),
        ViewEntry::Version(VersionEntry {
            order: seq(2),
            checkpoint: checkpoint_id(16),
            kind: CheckpointKind::OutsideEdits,
            thread: None,
            turn: None,
            ts: Timestamp::new(1_760_000_000_000).unwrap(),
            files: file_changes(),
        }),
    ]
}
