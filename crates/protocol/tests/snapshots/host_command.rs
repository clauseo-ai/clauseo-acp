//! Golden snapshots of `HostCommand`: one per variant, and the `RefreshLimits` extras. A child
//! of `snapshots.rs`, so its snapshots keep their names and their place in `snapshots/`.

use super::*;

golden! { HostCommand::name_of {
    CreateMatter => create_matter: HostCommand::CreateMatter {
        name: title("Smith v Jones"),
        source: MatterSource::NewFolder { folder: folder_ref_id(1) },
        client: Some(ClientName::new("Rathi Steels").unwrap()),
        next_date: Some("2026-11-20".parse::<Date>().unwrap()),
    };
    RelocateMatter => relocate_matter: HostCommand::RelocateMatter {
        matter: matter_id(1),
        folder: folder_ref_id(2),
    };
    UpdateMatter => update_matter: HostCommand::UpdateMatter {
        matter: matter_id(1),
        client: FieldChange::Set(ClientName::new("Rathi Steels").unwrap()),
        next_date: FieldChange::Clear,
        pinned: Some(true),
    };
    OpenMatter => open_matter: HostCommand::OpenMatter { matter: matter_id(1) };
    ForgetMatter => forget_matter: HostCommand::ForgetMatter { matter: matter_id(1) };
    MarkSeen => mark_seen: HostCommand::MarkSeen { thread: thread_id(2) };
    SnoozeThread => snooze_thread: HostCommand::SnoozeThread {
        thread: thread_id(2),
        until: Some(Timestamp::new(1_760_086_400_000).unwrap()),
    };
    MarkThreadDone => mark_thread_done: HostCommand::MarkThreadDone {
        thread: thread_id(2),
        done: true,
    };
    SweepDone => sweep_done: HostCommand::SweepDone {
        day_start: Timestamp::new(1_759_968_000_000).unwrap(),
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
        text: "Shorter".to_owned(),
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
        placeholder: false,
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
    SetUiSettings => set_ui_settings: HostCommand::SetUiSettings {
        patch: UiPatch {
            theme: Some(Theme::Dark),
            font_size: Some(18),
            lite: Some(Lite::On),
            ..UiPatch::default()
        },
    };
    SetToolSettings => set_tool_settings: HostCommand::SetToolSettings {
        patch: ToolsPatch {
            web_search: Some(false),
            browser: Some(true),
        },
    };
    SetPrivacySettings => set_privacy_settings: HostCommand::SetPrivacySettings {
        patch: privacy::privacy_patch(),
    };
    FirstAct => first_act: HostCommand::FirstAct { counts: true };
    RefreshLimits => refresh_limits: HostCommand::RefreshLimits {
        agent: Some(AgentKind::Claude),
        why: RefreshWhy::Shown,
    };
    ScanFolder => scan_folder: HostCommand::ScanFolder { folder: folder_ref_id(2) };
    CancelScan => cancel_scan: HostCommand::CancelScan { folder: folder_ref_id(2) };
    InstallBrowser => install_browser: HostCommand::InstallBrowser {};
    CancelBrowserInstall => cancel_browser_install: HostCommand::CancelBrowserInstall {};
} }

/// `RefreshLimits` for every agent (no `agent` field), from a "Try again".
#[test]
fn refresh_limits_every_agent() {
    let command = HostCommand::RefreshLimits {
        agent: None,
        why: RefreshWhy::TryAgain,
    };
    snapshot!("refresh_limits_every_agent", command);
    round_trips(&command);
}

/// Each `RefreshWhy` value, in order.
#[test]
fn refresh_why() {
    let whys = [RefreshWhy::Shown, RefreshWhy::TryAgain];
    snapshot!("refresh_why", whys);
    round_trips(&whys);
}
