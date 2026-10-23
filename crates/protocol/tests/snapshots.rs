//! Golden snapshots: the JSON form of every wire type, one canonical example per variant.
//!
//! The `golden!` macro writes one test per variant, named after it in `snake_case` (its snapshot
//! has the same name), and the match that names each variant's test. That match has no wildcard
//! arm, so a new variant fails to compile until it has a row, and that row carries its example.
//! Each example also decodes back to itself, in JSON and `MessagePack`.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]
#![deny(clippy::wildcard_enum_match_arm)]

#[cfg(test)]
#[path = "support/fixtures.rs"]
pub mod fixtures;

#[cfg(test)]
#[path = "support/examples.rs"]
pub mod examples;

#[cfg(test)]
#[path = "support/entries.rs"]
pub mod entries;

use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AppendTarget, AtLimitBehaviour, Billing, ConfigValue,
    ContentBlock, Cursor, DeliveryRequest, DomainEvent, ElicitationResponse, EntryId, Envelope,
    ErrorCode, FileChoice, FileDecision, HostCommand, HostError, HostEvent, LimitSource,
    LimitWindow, MatterSource, PatchBatch, PlanLabel, PromptBlock, RestoreRequest, StreamHeader,
    StreamId, TextOrBlob, Timestamp, ToolCallContent, ToolLocation, ViewEntry, ViewEpoch,
    ViewPatch, ViewPatchBody, ViewWindow, WindowKind,
};
use entries::{agent_text_entry, every_entry, header_entry};
use examples::{acp_unsupported_block, every_event};
use fixtures::{
    agent, checkpoint_id, choice, count, elicitation_id, event_id, file_ref_id, finite,
    folder_ref_id, matter_id, message_id, path, permission_request_id, raw, round_trips, seq,
    short, thread_id, title, turn_id, unknown_event, unknown_of, v7,
};
use fixtures::{blob as sample_blob, envelope as sample_envelope};

/// The example of the known kind `kind`, from `examples::every_event`.
#[cfg(test)]
fn event(kind: &str) -> DomainEvent {
    every_event()
        .into_iter()
        .find(|event| event.kind() == kind)
        .unwrap_or_else(|| panic!("no example of the kind {kind}"))
}

/// Writes one golden test per row, named after the variant's `snake_case` name, and the function
/// `$names` that names each variant's test. The match in `$names` lists every variant with no
/// wildcard arm, so a variant without a row does not compile.
macro_rules! golden {
    ($ty:ident :: $names:ident { $( $variant:ident => $test:ident : $example:expr ; )+ }) => {
        /// The name of the golden test of each variant.
        fn $names(value: &$ty) -> &'static str {
            match value {
                $( $ty::$variant { .. } => stringify!($test), )+
            }
        }

        $(
            #[test]
            fn $test() {
                insta::assert_json_snapshot!(stringify!($test), $example);
                round_trips(&$example);
            }
        )+

        /// Each row's example is a value of the variant the row names.
        #[test]
        fn each_example_is_its_variant() {
            $( assert_eq!($names(&$example), stringify!($test)); )+
        }
    };
}

/// A view epoch from `v7(n)`.
#[cfg(test)]
fn view_epoch(n: u64) -> ViewEpoch {
    ViewEpoch::new(v7(n)).unwrap()
}

/// A window with a header and one entry, as a `Reset` carries it.
#[cfg(test)]
fn view_window() -> ViewWindow {
    ViewWindow {
        header: Some(StreamHeader::Thread(header_entry())),
        entries: vec![ViewEntry::AgentText(agent_text_entry())],
        has_older: true,
    }
}

/// An account's limits, with one window.
#[cfg(test)]
fn account_limits() -> AccountLimits {
    AccountLimits {
        agent: AgentKind::Claude,
        account: AccountRef("acct-1".to_owned()),
        plan: Some(PlanLabel("Max".to_owned())),
        billing: Billing::Plan,
        windows: vec![LimitWindow {
            id: "five_hour".to_owned(),
            kind: WindowKind::Session,
            label: "5-hour".to_owned(),
            used_percent: finite(82.5),
            resets_at: Some(Timestamp::new(1_760_000_000_000).unwrap()),
            duration_mins: Some(300),
            warning: true,
        }],
        at_limit: AtLimitBehaviour::Pauses,
        source: LimitSource::Fresh {
            checked_at: Timestamp::new(1_760_000_000_000).unwrap(),
        },
    }
}

/// A `FilesChanged`-free patch batch with one `AppendText` patch.
#[cfg(test)]
fn patch_batch() -> PatchBatch {
    PatchBatch {
        stream: StreamId::Thread(thread_id(2)),
        epoch: view_epoch(50),
        patches: vec![ViewPatch {
            cursor: Cursor::new(1).unwrap(),
            body: ViewPatchBody::AppendText {
                message_id: message_id(9),
                target: AppendTarget::AgentText,
                from_len: 9,
                text: " ends".to_owned(),
            },
        }],
    }
}

mod domain_event {
    use super::*;

    golden! { DomainEvent::name_of {
        ThreadCreated => thread_created: event("thread_created");
        ThreadRenamed => thread_renamed: event("thread_renamed");
        ThreadArchived => thread_archived: event("thread_archived");
        AgentSessionAttached => agent_session_attached: event("agent_session_attached");
        AgentSessionDetached => agent_session_detached: event("agent_session_detached");
        UserMessage => user_message: event("user_message");
        TurnQueued => turn_queued: event("turn_queued");
        QueuedPromptEdited => queued_prompt_edited: event("queued_prompt_edited");
        SteerResolved => steer_resolved: event("steer_resolved");
        TurnStarted => turn_started: event("turn_started");
        TextAppended => text_appended: event("text_appended");
        ContentAppended => content_appended: event("content_appended");
        ToolCallStarted => tool_call_started: event("tool_call_started");
        ToolCallUpdated => tool_call_updated: event("tool_call_updated");
        PermissionRequested => permission_requested: event("permission_requested");
        PermissionResolved => permission_resolved: event("permission_resolved");
        PermissionExpired => permission_expired: event("permission_expired");
        ElicitationRequested => elicitation_requested: event("elicitation_requested");
        ElicitationResolved => elicitation_resolved: event("elicitation_resolved");
        ElicitationExpired => elicitation_expired: event("elicitation_expired");
        PlanReplaced => plan_replaced: event("plan_replaced");
        ModeChanged => mode_changed: event("mode_changed");
        ConfigOptionsReplaced => config_options_replaced: event("config_options_replaced");
        CommandsAvailable => commands_available: event("commands_available");
        UsageUpdated => usage_updated: event("usage_updated");
        Notice => notice: event("notice");
        CompactionChanged => compaction_changed: event("compaction_changed");
        FilesChanged => files_changed: event("files_changed");
        TurnFailed => turn_failed: event("turn_failed");
        TurnStoppedAtLimit => turn_stopped_at_limit: event("turn_stopped_at_limit");
        TurnEnded => turn_ended: event("turn_ended");
        StepUndone => step_undone: event("step_undone");
        MatterCreated => matter_created: event("matter_created");
        CheckpointRecorded => checkpoint_recorded: event("checkpoint_recorded");
        OutsideEdits => outside_edits: event("outside_edits");
        VersionRestored => version_restored: event("version_restored");
        Unknown => unknown: unknown_event("later_kind", raw(r#"{"z":1,"a":[true]}"#));
    } }

    /// The examples of known kinds are exactly `DomainEvent::KINDS`, in order.
    #[test]
    fn kinds_equal_known_kinds() {
        let events = every_event();
        let kinds: Vec<&str> = events.iter().map(DomainEvent::kind).collect();
        assert_eq!(
            kinds,
            DomainEvent::KINDS,
            "the examples' kinds (left) and DomainEvent::KINDS (right): add an example of each new \
             kind to every_event in tests/support/examples.rs, in KINDS order (see the checklist \
             at the end of Freeze rules in docs/decisions/0002-event-schema.md)"
        );
    }
}

mod host_command {
    use super::*;

    golden! { HostCommand::name_of {
        CreateMatter => create_matter: HostCommand::CreateMatter {
            name: title("Smith v Jones"),
            source: MatterSource::NewFolder { folder: folder_ref_id(1) },
        };
        CreateThread => create_thread: HostCommand::CreateThread {
            matter: matter_id(1),
            agent: agent("claude-acp"),
        };
        SendPrompt => send_prompt: HostCommand::SendPrompt {
            thread: thread_id(2),
            content: vec![
                PromptBlock::Text { text: "Summarise".to_owned() },
                PromptBlock::File { file: file_ref_id(3) },
            ],
            delivery: DeliveryRequest::SendNow,
        };
        EditQueuedPrompt => edit_queued_prompt: HostCommand::EditQueuedPrompt {
            thread: thread_id(2),
            turn: turn_id(3),
            content: vec![PromptBlock::Text { text: "Shorter".to_owned() }],
        };
        RemoveQueuedPrompt => remove_queued_prompt: HostCommand::RemoveQueuedPrompt {
            thread: thread_id(2),
            turn: turn_id(3),
        };
        SendQueuedNow => send_queued_now: HostCommand::SendQueuedNow {
            thread: thread_id(2),
            turn: turn_id(3),
        };
        ResumeQueue => resume_queue: HostCommand::ResumeQueue { thread: thread_id(2) };
        Cancel => cancel: HostCommand::Cancel { thread: thread_id(2) };
        RestartAgent => restart_agent: HostCommand::RestartAgent { thread: thread_id(2) };
        AnswerPermission => answer_permission: HostCommand::AnswerPermission {
            thread: thread_id(2),
            request: permission_request_id(10),
            option_id: choice("allow"),
        };
        AnswerElicitation => answer_elicitation: HostCommand::AnswerElicitation {
            thread: thread_id(2),
            elicitation: elicitation_id(12),
            response: ElicitationResponse::Accept { values: raw(r#"{"party":"Jones"}"#) },
        };
        SetConfigOption => set_config_option: HostCommand::SetConfigOption {
            thread: thread_id(2),
            option_id: choice("model"),
            value: ConfigValue::Select(choice("opus")),
        };
        SetMode => set_mode: HostCommand::SetMode {
            thread: thread_id(2),
            mode_id: choice("plan"),
        };
        RenameThread => rename_thread: HostCommand::RenameThread {
            thread: thread_id(2),
            title: title("Draft SPA v2"),
        };
        ArchiveThread => archive_thread: HostCommand::ArchiveThread { thread: thread_id(2) };
        UndoStep => undo_step: HostCommand::UndoStep {
            thread: thread_id(2),
            turn: turn_id(3),
            files: vec![FileDecision { path: path("a.docx"), choice: FileChoice::Undo }],
        };
        RestoreVersion => restore_version: HostCommand::RestoreVersion {
            matter: matter_id(1),
            checkpoint: checkpoint_id(17),
            scope: RestoreRequest::Files { paths: vec![path("a.docx")] },
        };
    } }
}

mod view_patch {
    use super::*;

    golden! { ViewPatchBody::name_of {
        Upsert => upsert: ViewPatchBody::Upsert {
            entry: Box::new(ViewEntry::AgentText(agent_text_entry())),
        };
        Remove => remove: ViewPatchBody::Remove { entry_id: EntryId::Notice(event_id(40)) };
        AppendText => append_text: ViewPatchBody::AppendText {
            message_id: message_id(9),
            target: AppendTarget::AgentText,
            from_len: 9,
            text: " ends".to_owned(),
        };
        Reset => reset: ViewPatchBody::Reset { view_window: Box::new(view_window()) };
    } }

    /// `AppendText` into the second block of a user message.
    #[test]
    fn append_text_to_block() {
        let patch = ViewPatchBody::AppendText {
            message_id: message_id(5),
            target: AppendTarget::UserBlock(1),
            from_len: 4096,
            text: "the rest of the clause".to_owned(),
        };
        insta::assert_json_snapshot!("append_text_to_block", patch);
        round_trips(&patch);
    }
}

mod view_entry {
    use super::*;

    /// The example of the entry whose serde key is `key`, from `entries::every_entry`.
    #[cfg(test)]
    fn entry(key: &str) -> ViewEntry {
        every_entry()
            .into_iter()
            .find(|entry| serde_key(entry) == key)
            .unwrap_or_else(|| panic!("no example of the entry {key}"))
    }

    /// The key of the entry's one-key JSON object.
    #[cfg(test)]
    fn serde_key(entry: &ViewEntry) -> String {
        let json = serde_json::to_value(entry).unwrap();
        let object = json.as_object().unwrap();
        assert_eq!(object.len(), 1, "{json}");
        object.keys().next().unwrap().clone()
    }

    golden! { ViewEntry::name_of {
        Header => header: entry("header");
        UserMessage => user_message: entry("user_message");
        AgentText => agent_text: entry("agent_text");
        AgentContent => agent_content: entry("agent_content");
        ToolCall => tool_call: entry("tool_call");
        Permission => permission: entry("permission");
        Elicitation => elicitation: entry("elicitation");
        Step => step: entry("step");
        LimitStop => limit_stop: entry("limit_stop");
        Notice => notice: entry("notice");
        Marker => marker: entry("marker");
        MatterHeader => matter_header: entry("matter_header");
        Version => version: entry("version");
    } }
}

mod host_event {
    use super::*;

    golden! { HostEvent::name_of {
        Patches => patches: HostEvent::Patches(patch_batch());
        Events => events: HostEvent::Events(vec![sample_envelope(event("turn_started"))]);
        CaughtUp => caught_up: HostEvent::CaughtUp {
            stream: StreamId::Thread(thread_id(2)),
            last_seq: seq(7),
        };
        AccountLimitsChanged => account_limits_changed: HostEvent::AccountLimitsChanged(
            account_limits(),
        );
    } }
}

mod text_or_blob {
    use super::*;

    golden! { TextOrBlob::name_of {
        Inline => inline: TextOrBlob::Inline("The lease \"ends\"\n".to_owned());
        Blob => blob: TextOrBlob::Blob(sample_blob(7, 5000));
    } }
}

mod tool_call_content {
    use super::*;

    golden! { ToolCallContent::name_of {
        Text => text: ToolCallContent::Text { text: TextOrBlob::Inline("done".to_owned()) };
        Content => content: ToolCallContent::Content {
            content: ContentBlock::Text { text: "Clause 4 applies.".to_owned() },
        };
        Diff => diff: ToolCallContent::Diff {
            path: ToolLocation::Inside { path: path("a.md"), line: None },
            old: Some(TextOrBlob::Inline("old\n".to_owned())),
            new: TextOrBlob::Blob(sample_blob(8, 9000)),
        };
        Unknown => unknown: unknown_of::<ToolCallContent>("terminal");
    } }
}

mod content_block {
    use super::*;

    golden! { ContentBlock::name_of {
        Text => text: ContentBlock::Text { text: "Summarise".to_owned() };
        Image => image: ContentBlock::Image {
            data: TextOrBlob::Inline("iVBORw0KGgo=".to_owned()),
            mime_type: short("image/png"),
            uri: None,
        };
        Audio => audio: ContentBlock::Audio {
            data: TextOrBlob::Blob(sample_blob(9, 4_000_000)),
            mime_type: short("audio/mpeg"),
        };
        ResourceLink => resource_link: ContentBlock::ResourceLink {
            uri: "file:///matter/a.pdf".to_owned(),
            name: "a.pdf".to_owned(),
            mime_type: Some(short("application/pdf")),
            title: None,
            description: Some("The lease".to_owned()),
            size: Some(count(2048)),
        };
        Resource => resource: ContentBlock::Resource {
            uri: "file:///matter/b.md".to_owned(),
            mime_type: Some(short("text/markdown")),
            text: Some(TextOrBlob::Inline("# Notes".to_owned())),
            blob: None,
        };
        Unsupported => unsupported: acp_unsupported_block();
    } }
}

mod host_error {
    use super::*;

    #[test]
    fn host_error() {
        let error = HostError::new(ErrorCode::ThreadQueueFull, "10 turns are already queued");
        insta::assert_json_snapshot!("host_error", error);
    }
}

#[test]
fn envelope() {
    let value: Envelope = sample_envelope(event("turn_started"));
    insta::assert_json_snapshot!("envelope", value);
}
