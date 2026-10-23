//! One example of every event kind and of the larger payloads, built from the fixtures' ids.
//!
//! A test file that uses it includes it next to `fixtures`, which it builds on:
//!
//! ```text
//! #[cfg(test)]
//! #[path = "support/examples.rs"]
//! pub mod examples;
//! ```

use clauseo_protocol::{
    AgentCapabilities, AgentId, AgentSessionAttached, AgentSessionDetached, AvailableCommand,
    ChangeKind, CheckpointKind, CheckpointRecorded, CommandsAvailable, CompactionChanged,
    CompactionStatus, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind,
    ConfigOptionsReplaced, ContentAppended, ContentBlock, ContentId, Cost, Deliverable, Delivery,
    DetachReason, DomainEvent, ElicitationExpired, ElicitationOutcome, ElicitationRequest,
    ElicitationRequested, ElicitationResolved, ErrorCode, ExpiryReason, FileChange, FilesChanged,
    ImportSummary, LimitStopReason, MatterCreated, MatterKind, ModeChanged, Notice, NoticeLevel,
    OutsideEdits, PermissionChoice, PermissionExpired, PermissionKind, PermissionOption,
    PermissionRequested, PermissionResolved, PlanEntry, PlanPriority, PlanReplaced, PlanStatus,
    Principal, PromptCapabilities, QueuedPromptEdited, RawFrom, RenameSource, RestoreScope,
    SessionCapabilities, SkippedFile, SkippedFileReason, SteerOutcome, SteerResolved,
    SteeringCapability, StepUndone, StopReason, TextAppended, TextKind, TextOrBlob, ThreadArchived,
    ThreadCreated, ThreadRenamed, Timestamp, ToolCallContent, ToolCallPatch, ToolCallStarted,
    ToolCallStatus, ToolCallUpdated, ToolKind, ToolLocation, TurnEnded, TurnFailed, TurnQueued,
    TurnStarted, TurnStoppedAtLimit, TurnUsage, UsageUpdated, UserMessage, VersionRestored,
};

use crate::fixtures::{
    checkpoint_id, count, elicitation_id, finite, matter_id, message_id, path,
    permission_request_id, raw, short, thread_id, tool_call_id, turn_id,
};

/// A tool call with every field filled.
pub fn tool_call_started() -> ToolCallStarted {
    ToolCallStarted {
        turn: turn_id(3),
        tool_call: tool_call_id(4),
        acp_id: short("toolu_01"),
        title: short("Read the lease"),
        kind: ToolKind::Read,
        tool_name: Some(short("Read")),
        status: ToolCallStatus::InProgress,
        locations: vec![
            ToolLocation::Inside {
                path: path("leases/lease.docx"),
                line: Some(3),
            },
            ToolLocation::Outside {
                display: "/etc/hosts".to_owned(),
            },
        ],
        raw_input: Some(TextOrBlob::Inline(r#"{"path":"lease.docx"}"#.to_owned())),
        raw_output: None,
        content: vec![
            ToolCallContent::Text {
                text: TextOrBlob::Inline("done".to_owned()),
            },
            ToolCallContent::Diff {
                path: ToolLocation::Inside {
                    path: path("a.md"),
                    line: None,
                },
                old: None,
                new: TextOrBlob::Inline("new".to_owned()),
            },
        ],
    }
}

/// One file change of each shape.
pub fn file_changes() -> Vec<FileChange> {
    let id = |text: &str| Some(ContentId::new(text).unwrap());
    vec![
        FileChange::new(
            path("a.docx"),
            ChangeKind::Modified,
            id("ab12"),
            id("cd34"),
            count(2048),
        )
        .unwrap()
        .with_lawyer_edits(),
        FileChange::new(
            path("b/new.docx"),
            ChangeKind::Renamed {
                from: path("b/old.docx"),
            },
            id("ab12"),
            id("ab12"),
            count(10),
        )
        .unwrap(),
    ]
}

/// One example of every known event kind, in `DomainEvent::KINDS` order.
pub fn every_event() -> Vec<DomainEvent> {
    [
        thread_events(),
        streaming_events(),
        session_events(),
        turn_end_events(),
        matter_events(),
    ]
    .concat()
}

/// Thread lifecycle and queue events.
fn thread_events() -> Vec<DomainEvent> {
    let turn = turn_id(3);
    let message = message_id(5);
    let capabilities = AgentCapabilities {
        load_session: true,
        prompt: PromptCapabilities {
            image: true,
            audio: false,
            embedded_context: true,
        },
        session: SessionCapabilities {
            list: true,
            resume: true,
            close: false,
        },
        steering: Some(SteeringCapability {
            native: true,
            idle_fallback_to_host: false,
        }),
    };
    vec![
        ThreadCreated {
            matter: matter_id(1),
            agent: AgentId::new("claude-acp").unwrap(),
            title: Some(short("Draft SPA")),
        }
        .into(),
        ThreadRenamed {
            title: short("Draft SPA v2"),
            by: RenameSource::Agent,
        }
        .into(),
        ThreadArchived {}.into(),
        AgentSessionAttached {
            session: "sess-1".parse().unwrap(),
            agent_version: Some("0.9.0".to_owned()),
            capabilities,
        }
        .into(),
        AgentSessionDetached {
            reason: DetachReason::Exited {
                code: None,
                stderr_tail: "killed".to_owned(),
            },
        }
        .into(),
        UserMessage {
            turn,
            message,
            acp_message_id: None,
            content: vec![ContentBlock::Text {
                text: "Summarise".to_owned(),
            }],
            delivery: Delivery::Steer,
        }
        .into(),
        TurnQueued::new(thread_id(2), turn, thread_id(2), Some(turn_id(6)))
            .unwrap()
            .into(),
        QueuedPromptEdited {
            turn,
            message,
            content: vec![],
        }
        .into(),
        SteerResolved {
            message,
            outcome: SteerOutcome::Requeued { turn: turn_id(7) },
        }
        .into(),
    ]
}

/// Turn start, streamed text and tool calls.
fn streaming_events() -> Vec<DomainEvent> {
    let turn = turn_id(3);
    let image = ContentBlock::Image {
        data: TextOrBlob::Inline("iVBORw0KGgo=".to_owned()),
        mime_type: short("image/png"),
        uri: None,
    };
    vec![
        TurnStarted {
            turn,
            checkpoint_before: checkpoint_id(8),
        }
        .into(),
        TextAppended {
            turn,
            message: message_id(9),
            acp_message_id: Some(short("msg_01")),
            kind: TextKind::Answer,
            text: "The lease \"ends\"\n".to_owned(),
        }
        .into(),
        Box::new(ContentAppended {
            turn,
            message: message_id(9),
            acp_message_id: None,
            kind: TextKind::Thought,
            block: image,
        })
        .into(),
        Box::new(tool_call_started()).into(),
        Box::new(ToolCallUpdated {
            tool_call: tool_call_id(4),
            patch: ToolCallPatch {
                status: Some(ToolCallStatus::Completed),
                content: Some(vec![]),
                ..ToolCallPatch::default()
            },
        })
        .into(),
    ]
}

/// Approvals and session state.
fn session_events() -> Vec<DomainEvent> {
    let select = ConfigOption {
        id: short("model"),
        name: short("Model"),
        description: None,
        category: ConfigCategory::Model,
        kind: ConfigOptionKind::Select {
            current_value: short("opus"),
            options: vec![ConfigChoice {
                value: short("opus"),
                name: short("Opus"),
                description: Some("Largest".to_owned()),
                group: None,
            }],
        },
    };
    vec![
        PermissionRequested {
            request: permission_request_id(10),
            tool_call: Some(tool_call_id(4)),
            options: vec![PermissionOption {
                option_id: short("allow"),
                kind: PermissionKind::AllowOnce,
                agent_label: "Yes".to_owned(),
            }],
            deliverable: Deliverable::Live,
        }
        .into(),
        PermissionResolved {
            request: permission_request_id(10),
            choice: PermissionChoice::Selected {
                option_id: short("allow"),
                kind: PermissionKind::AllowOnce,
            },
            by: Principal::LocalDesktop,
        }
        .into(),
        PermissionExpired {
            request: permission_request_id(11),
            reason: ExpiryReason::TurnEnded,
        }
        .into(),
        ElicitationRequested {
            elicitation: elicitation_id(12),
            request: ElicitationRequest::Form {
                message: "Which party?".to_owned(),
                schema: raw(r#"{"type":"object","properties":{"party":{"type":"string"}}}"#),
            },
        }
        .into(),
        ElicitationResolved {
            elicitation: elicitation_id(12),
            outcome: ElicitationOutcome::Accepted,
            by: Principal::LocalDesktop,
        }
        .into(),
        ElicitationExpired {
            elicitation: elicitation_id(13),
            reason: ExpiryReason::AgentExited,
        }
        .into(),
        PlanReplaced {
            entries: vec![PlanEntry {
                content: "Read the lease".to_owned(),
                priority: PlanPriority::High,
                status: PlanStatus::InProgress,
            }],
        }
        .into(),
        ModeChanged {
            mode_id: short("plan"),
        }
        .into(),
        ConfigOptionsReplaced {
            options: vec![select],
        }
        .into(),
        CommandsAvailable {
            commands: vec![AvailableCommand {
                name: short("review"),
                description: "Review a document".to_owned(),
                input_hint: None,
            }],
        }
        .into(),
    ]
}

/// Usage, notices, files and the end of a turn.
fn turn_end_events() -> Vec<DomainEvent> {
    let turn = turn_id(3);
    vec![
        UsageUpdated {
            context_used: Some(count(1200)),
            context_size: Some(count(200_000)),
            cost: Some(Cost {
                amount: finite(0.42),
                currency: "USD".to_owned(),
            }),
        }
        .into(),
        Notice {
            level: NoticeLevel::Warning,
            text: "Context nearly full".to_owned(),
        }
        .into(),
        CompactionChanged {
            status: CompactionStatus::Completed,
            summary: Some(TextOrBlob::Inline("So far".to_owned())),
        }
        .into(),
        FilesChanged {
            turn,
            files: file_changes(),
            skipped: vec![SkippedFile {
                path: path("big.zip"),
                reason: SkippedFileReason::OverSizeCap,
            }],
        }
        .into(),
        TurnFailed {
            turn,
            code: ErrorCode::AgentExited,
            detail: None,
        }
        .into(),
        TurnStoppedAtLimit {
            turn,
            window: Some("five_hour".to_owned()),
            resets_at: Some(Timestamp::new(1_760_000_000_000).unwrap()),
            reason: LimitStopReason::PlanWindow,
        }
        .into(),
        TurnEnded {
            turn,
            stop: StopReason::EndTurn,
            usage: Some(TurnUsage {
                total: count(30),
                input: count(20),
                output: count(10),
                thought: None,
                cached_read: Some(count(5)),
                cached_write: None,
            }),
            checkpoint_after: Some(checkpoint_id(14)),
        }
        .into(),
        StepUndone {
            turn,
            checkpoint: checkpoint_id(15),
            restored: vec![path("a.docx")],
            kept: vec![path("b/new.docx")],
        }
        .into(),
    ]
}

/// Matter-level events.
fn matter_events() -> Vec<DomainEvent> {
    let turn = turn_id(3);
    vec![
        MatterCreated::new(
            "Smith v Jones",
            MatterKind::Folder,
            Some(ImportSummary {
                files: 12,
                bytes: count(4_000_000),
                cloud_only: 1,
            }),
        )
        .unwrap()
        .into(),
        CheckpointRecorded::new(
            checkpoint_id(8),
            CheckpointKind::BeforeTurn,
            Some(thread_id(2)),
            Some(turn),
        )
        .unwrap()
        .into(),
        OutsideEdits {
            checkpoint: checkpoint_id(16),
            files: file_changes(),
        }
        .into(),
        VersionRestored {
            from: checkpoint_id(8),
            checkpoint: checkpoint_id(17),
            scope: RestoreScope::Files {
                paths: vec![path("a.docx")],
            },
        }
        .into(),
    ]
}

/// An unsupported block as `acp` keeps it.
pub fn acp_unsupported_block() -> ContentBlock {
    ContentBlock::Unsupported {
        kind: "hologram".to_owned(),
        raw_from: RawFrom::Acp,
        raw: Some(TextOrBlob::Inline(r#"{"type":"hologram"}"#.to_owned())),
    }
}
