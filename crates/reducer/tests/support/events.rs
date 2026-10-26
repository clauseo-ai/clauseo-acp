//! Event builders of the fixtures: one payload each, from ids the caller passes. Each file that
//! includes `fixtures.rs` declares this file beside it, and `fixtures.rs` re-exports all of it.

use clauseo_protocol::{
    ChangeKind, CheckpointId, CompactString, ContentBlock, ContentId, Count, Deliverable, Delivery,
    DomainEvent, FileChange, FilesChanged, MatterPath, MessageId, PermissionChoice, PermissionKind,
    PermissionOption, PermissionRequestId, PermissionRequested, PermissionResolved, PlanEntry,
    PlanPriority, PlanReplaced, PlanStatus, Principal, StopReason, TextAppended, TextKind,
    TextOrBlob, ThreadCreated, ToolCallContent, ToolCallId, ToolCallPatch, ToolCallStarted,
    ToolCallStatus, ToolCallUpdated, ToolKind, ToolLocation, TurnEnded, TurnId, TurnStarted,
    TurnUsage, UsageUpdated, UserMessage,
};

use super::fixtures::{Fx, Log, fixture_matter};

/// `len` bytes of ASCII legal prose, the same for the same `len`.
pub fn prose(len: usize) -> String {
    "The tenant shall keep the premises in good and substantial repair. "
        .chars()
        .cycle()
        .take(len)
        .collect()
}

/// A `CompactString`.
pub fn short(text: &str) -> CompactString {
    CompactString::from(text)
}

/// A `ThreadCreated` in the fixture matter.
pub fn thread_created() -> Fx<DomainEvent> {
    Ok(ThreadCreated {
        matter: fixture_matter()?,
        agent: "claude-acp".parse()?,
        title: Some(short("Indemnity review")),
    }
    .into())
}

/// A queued prompt of `text` opening `turn`.
pub fn user_message(turn: TurnId, message: MessageId, text: &str) -> DomainEvent {
    UserMessage {
        turn,
        message,
        acp_message_id: None,
        content: vec![ContentBlock::Text {
            text: text.to_owned(),
        }],
        delivery: Delivery::Queue,
    }
    .into()
}

/// A chunk of `kind` text onto `message`.
pub fn text(turn: TurnId, message: MessageId, kind: TextKind, text: String) -> DomainEvent {
    TextAppended {
        turn,
        message,
        acp_message_id: None,
        kind,
        text,
    }
    .into()
}

/// A running edit of `docs/n.md`.
pub fn tool_started(turn: TurnId, tool_call: ToolCallId, n: usize) -> Fx<DomainEvent> {
    let started = ToolCallStarted {
        turn,
        tool_call,
        acp_id: short(&format!("toolu_{n:03}")),
        title: short(&format!("Edit clause {n}")),
        kind: ToolKind::Edit,
        tool_name: Some(short("Edit")),
        status: ToolCallStatus::InProgress,
        locations: vec![ToolLocation::Inside {
            path: doc_path(n)?,
            line: Some(1),
        }],
        raw_input: None,
        raw_output: None,
        content: vec![],
    };
    Ok(Box::new(started).into())
}

/// An update setting the tool call's status.
pub fn tool_status(tool_call: ToolCallId, status: ToolCallStatus) -> DomainEvent {
    let patch = ToolCallPatch {
        status: Some(status),
        ..ToolCallPatch::default()
    };
    Box::new(ToolCallUpdated { tool_call, patch }).into()
}

/// An update replacing the tool call's content with a line of text and a diff of `docs/n.md`.
pub fn tool_diff(tool_call: ToolCallId, n: usize) -> Fx<DomainEvent> {
    let content = vec![
        ToolCallContent::Text {
            text: TextOrBlob::Inline(format!("Edited clause {n}.")),
        },
        ToolCallContent::Diff {
            path: ToolLocation::Inside {
                path: doc_path(n)?,
                line: None,
            },
            old: Some(TextOrBlob::Inline(prose(120))),
            new: TextOrBlob::Inline(prose(160)),
        },
    ];
    let patch = ToolCallPatch {
        content: Some(content),
        ..ToolCallPatch::default()
    };
    Ok(Box::new(ToolCallUpdated { tool_call, patch }).into())
}

/// The matter path `docs/n.md`.
pub fn doc_path(n: usize) -> Fx<MatterPath> {
    MatterPath::new(&format!("docs/{n}.md"))
}

/// A plan of three steps, the first `done` of them completed.
pub fn plan(done: usize) -> DomainEvent {
    let entries = (0..3)
        .map(|step| PlanEntry {
            content: format!("Step {step} of the review"),
            priority: PlanPriority::Medium,
            status: if step < done {
                PlanStatus::Completed
            } else {
                PlanStatus::Pending
            },
        })
        .collect();
    PlanReplaced { entries }.into()
}

/// A live request to allow `tool_call` once.
pub fn permission_requested(
    request: PermissionRequestId,
    tool_call: Option<ToolCallId>,
) -> DomainEvent {
    PermissionRequested {
        request,
        tool_call,
        options: vec![PermissionOption {
            option_id: short("allow"),
            kind: PermissionKind::AllowOnce,
            agent_label: "Yes".to_owned(),
        }],
        deliverable: Deliverable::Live,
    }
    .into()
}

/// The lawyer allowing `request` once.
pub fn permission_resolved(request: PermissionRequestId) -> DomainEvent {
    PermissionResolved {
        request,
        choice: PermissionChoice::Selected {
            option_id: short("allow"),
            kind: PermissionKind::AllowOnce,
        },
        by: Principal::LocalDesktop,
    }
    .into()
}

/// Context usage of `used` tokens out of 200,000.
pub fn usage(used: u64) -> Fx<DomainEvent> {
    Ok(UsageUpdated {
        context_used: Some(Count::new(used)?),
        context_size: Some(Count::new(200_000)?),
        cost: None,
    }
    .into())
}

/// One modified and one added file.
pub fn file_changes() -> Fx<Vec<FileChange>> {
    let content = |text: &str| ContentId::new(text).map(Some);
    Ok(vec![
        FileChange::new(
            MatterPath::new("lease.docx")?,
            ChangeKind::Modified,
            content("ab12")?,
            content("cd34")?,
            Count::new(2048)?,
        )?,
        FileChange::new(
            MatterPath::new("notes/summary.md")?,
            ChangeKind::Added,
            None,
            content("ef56")?,
            Count::new(300)?,
        )?,
    ])
}

/// `turn`'s files: [`file_changes`].
pub fn files_changed(turn: TurnId) -> Fx<DomainEvent> {
    Ok(FilesChanged {
        turn,
        files: file_changes()?,
        skipped: vec![],
    }
    .into())
}

/// `turn` ending normally, with usage, after `checkpoint_after`.
pub fn turn_ended(turn: TurnId, checkpoint_after: CheckpointId) -> Fx<DomainEvent> {
    Ok(TurnEnded {
        turn,
        stop: StopReason::EndTurn,
        usage: Some(TurnUsage {
            total: Count::new(30)?,
            input: Count::new(20)?,
            output: Count::new(10)?,
            thought: None,
            cached_read: None,
            cached_write: None,
        }),
        checkpoint_after: Some(checkpoint_after),
    }
    .into())
}

/// Pushes a prompt and its `TurnStarted`, and returns the new turn.
pub fn start_turn(log: &mut Log, prompt: &str) -> Fx<TurnId> {
    let turn: TurnId = log.fresh()?;
    let message = log.fresh()?;
    log.push(user_message(turn, message, prompt))?;
    let checkpoint_before = log.fresh()?;
    log.push(TurnStarted {
        turn,
        checkpoint_before,
    })?;
    Ok(turn)
}
