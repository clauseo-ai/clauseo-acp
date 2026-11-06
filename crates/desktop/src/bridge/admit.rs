//! What a window may send to the host, by its label (`Bridge::admit`). `main` gets the
//! allow-list of decision record 0019 (`admit_main`: one arm per host command and no catch-all),
//! and the settings window its own settings commands only (0028, U12): `SetUiSettings`,
//! `SetToolSettings`, `SetPrivacySettings` and `RefreshLimits`, the last throttled in
//! `dispatch_at`. Any other window sends nothing. Of the usage counts' two commands (0036), each
//! window gets the one its screen sends: `main` the welcome's `FirstAct`, the settings window
//! the privacy switches. Of the updater's four (0037), `main` sends `InstallUpdate`,
//! `CancelScheduledUpdate` and `CheckForUpdate` (its "Restart to update" row), and the settings
//! window `SetUpdateMode` and `CheckForUpdate`: settings never installs, main never changes the
//! mode. Of the connector commands (R24, `connector_main` and `connector_settings`), `main`
//! sends only `DismissCaseLawHint` and `RefreshBalance`: it shows agent output, so it never adds
//! a connector or a key, spends credits, opens a pay link, signs out or removes; its hint and
//! banner open Settings at the connector (`open_settings` with `section: "connectors"`), whose
//! window sends the rest.

use clauseo_protocol::{ConnectorCommand, FolderRefId, HostCommand, HostError, MatterSource};

use super::{Bridge, SETTINGS_WINDOW, invalid, lock};

/// Whether the window `label` may send `command`: `main` what `admit_main` admits, the
/// `settings` window `SetUiSettings`, `SetToolSettings`, `SetPrivacySettings`, for its Skills
/// pane `SetSkillPack` (0040), for its
/// Plan and usage `RefreshLimits` (0028, U12; `dispatch_at` throttles it), for its
/// updates row `SetUpdateMode` and `CheckForUpdate` (0037), and for its Connectors section
/// the connector commands but the hint's (R24), any other window nothing.
pub(super) fn admit(bridge: &Bridge, label: &str, command: &HostCommand) -> Result<(), HostError> {
    match label {
        crate::MAIN_WINDOW => admit_main(bridge, command),
        SETTINGS_WINDOW => match command {
            // Plan and usage sends `Shown` as it opens and `TryAgain` from its button.
            // Privacy (UF): the switches, which before the first act are the first act too.
            HostCommand::SetUiSettings { .. }
            | HostCommand::SetToolSettings { .. }
            | HostCommand::SetPrivacySettings { .. }
            // Skills (0040, PR-E): turning a pack on or off is a setting; the id is checked
            // by its type, the pack by the host.
            | HostCommand::SetSkillPack { .. }
            | HostCommand::RefreshLimits { .. }
            // Updates (0037): the mode and "Check now"; installing is the main window's.
            | HostCommand::SetUpdateMode { .. }
            | HostCommand::CheckForUpdate {} => Ok(()),
            HostCommand::Connector(connector) => connector_settings(connector),
            _ => Err(invalid(
                "the settings window sends only set_ui_settings, set_tool_settings, \
                 set_privacy_settings, set_skill_pack, refresh_limits, set_update_mode, \
                 check_for_update and the connector commands",
            )),
        },
        _ => Err(invalid(format!("window {label} may send no command"))),
    }
}

/// Whether `main` may send `command`: an allow-list, one arm per host command and no
/// catch-all, so a command the host adds does not compile here until the bridge rules on it
/// (`docs/decisions/0019-app-stream-to-the-window.md`). Refuses what goes another way
/// (`AnswerPermission`), what may not come from the window at all (`SetMode`,
/// `SetConfigOption`), and a `CreateMatter`, `RelocateMatter` or `ScanFolder` whose folder
/// ref this run's `pick_folder` did not issue. `CancelScan` always passes: it starts nothing,
/// and cancelling a scan that is not running does nothing.
fn admit_main(bridge: &Bridge, command: &HostCommand) -> Result<(), HostError> {
    match command {
        HostCommand::CreateMatter { source, .. } => picked_source(bridge, source),
        // Both read the folder the ref names: only one this run's picker issued.
        HostCommand::RelocateMatter { folder, .. } | HostCommand::ScanFolder { folder } => {
            picked(bridge, *folder)
        }
        // `UpdateMatter`, `OpenMatter` and `ForgetMatter` name a matter only.
        HostCommand::UpdateMatter { .. }
        | HostCommand::OpenMatter { .. }
        | HostCommand::ForgetMatter { .. }
        | HostCommand::CreateThread { .. }
        | HostCommand::SendPrompt { .. }
        | HostCommand::EditQueuedPrompt { .. }
        | HostCommand::RemoveQueuedPrompt { .. }
        | HostCommand::SendQueuedNow { .. }
        | HostCommand::ResumeQueue { .. }
        | HostCommand::Cancel { .. }
        | HostCommand::RestartAgent { .. }
        | HostCommand::AnswerElicitation { .. }
        | HostCommand::RenameThread { .. }
        | HostCommand::ArchiveThread { .. }
        // H6: names a thread only.
        | HostCommand::MarkSeen { .. }
        // The rail's triage: a thread and a time or a flag; the morning sweep a time.
        | HostCommand::SnoozeThread { .. }
        | HostCommand::MarkThreadDone { .. }
        | HostCommand::SweepDone { .. }
        | HostCommand::UndoStep { .. }
        | HostCommand::RestoreVersion { .. }
        | HostCommand::SetUiSettings { .. }
        | HostCommand::CancelScan { .. }
        // The welcome's button (UD1), and QF.11's notice for installs from before S21 (0036).
        | HostCommand::FirstAct { .. }
        // The window's "Try again" sends it; a window's focus sends `Shown` from Rust
        // (`Bridge::shown`), not through here.
        | HostCommand::RefreshLimits { .. }
        // The Chrome for Testing download (0029): the main window's consent dialog sends
        // it after the lawyer agrees, and its Cancel button the cancel. No field to check.
        | HostCommand::InstallBrowser {}
        | HostCommand::CancelBrowserInstall {}
        // R23: the rail card's update actions, "Restart to update" (now or after the answer),
        // taking a scheduled install back, and "Check again"; settings may not install (0037).
        | HostCommand::InstallUpdate { .. }
        | HostCommand::CancelScheduledUpdate {}
        | HostCommand::CheckForUpdate {} => Ok(()),
        HostCommand::AnswerPermission { .. } => Err(invalid(
            "a permission is answered with answer_permission, not dispatch",
        )),
        // A mode or option can widen what the agent may do without asking (Claude's
        // bypassPermissions), an escalation 0001 puts behind a native dialog drawn by Rust.
        // That path does not exist yet, and the window never sends either.
        HostCommand::SetMode { .. } | HostCommand::SetConfigOption { .. } => Err(invalid(
            "modes and config options are not settable from the window yet",
        )),
        // The tool switches live in the settings window only; the main window shows the
        // tools and offers to open Settings, so it has no caller (least privilege).
        HostCommand::SetToolSettings { .. } => Err(invalid(
            "the tool switches are set from the settings window only",
        )),
        // The privacy switches live in the settings window only (UF's privacy page), as the
        // tool switches do (least privilege, 0036).
        HostCommand::SetPrivacySettings { .. } => Err(invalid(
            "the privacy switches are set from the settings window only",
        )),
        // The mode lives in the settings window only, as the other switches do.
        HostCommand::SetUpdateMode { .. } => Err(invalid(
            "the update mode is set from the settings window only",
        )),
        // Skill packs are turned on and off in the settings window's Skills pane only, as the
        // other switches are (least privilege, 0040); the matter picker only reads them.
        HostCommand::SetSkillPack { .. } => Err(invalid(
            "skill packs are turned on and off from the settings window only",
        )),
        // Decision record 0051: the host does not run them yet; its next step rules here.
        HostCommand::RetryProtection {} | HostCommand::RemoveSandboxSetup {} => Err(invalid(
            "the protection commands are not admitted yet",
        )),
        // R24: the room's hint and the balance only; the rest is the settings window's.
        HostCommand::Connector(connector) => connector_main(connector),
    }
}

/// The connector commands `main` may send (R24, the app review of #470): dismissing the case-law
/// hint and refreshing the balance. One arm per command and no catch-all, as in `admit_main`.
fn connector_main(command: &ConnectorCommand) -> Result<(), HostError> {
    match command {
        ConnectorCommand::DismissCaseLawHint { .. } | ConnectorCommand::RefreshBalance { .. } => {
            Ok(())
        }
        ConnectorCommand::Check {}
        | ConnectorCommand::Add { .. }
        | ConnectorCommand::AddApiKey { .. }
        | ConnectorCommand::AddCredits { .. }
        | ConnectorCommand::SignOut { .. }
        | ConnectorCommand::Remove { .. }
        | ConnectorCommand::OpenPayLink { .. } => Err(invalid(
            "the main window sends only dismiss_case_law_hint and refresh_balance of the \
             connector commands; the rest are sent from Settings",
        )),
    }
}

/// The connector commands the settings window's Connectors section may send: all but the
/// room's hint, which it never shows. One arm per command and no catch-all.
fn connector_settings(command: &ConnectorCommand) -> Result<(), HostError> {
    match command {
        ConnectorCommand::Check {}
        | ConnectorCommand::Add { .. }
        | ConnectorCommand::AddApiKey { .. }
        | ConnectorCommand::AddCredits { .. }
        | ConnectorCommand::SignOut { .. }
        | ConnectorCommand::Remove { .. }
        | ConnectorCommand::RefreshBalance { .. }
        // The pay link the host holds, behind the native confirm; the room's banner opens
        // Settings at the connector, which sends it.
        | ConnectorCommand::OpenPayLink { .. } => Ok(()),
        ConnectorCommand::DismissCaseLawHint { .. } => Err(invalid(
            "the case-law hint is dismissed from the main window",
        )),
    }
}

/// Whether a `CreateMatter`'s or a `FolderSupport` query's `source` names a folder ref this
/// run's `pick_folder` issued: `command.invalid` when it does not, so the window can't probe
/// other refs.
pub(super) fn picked_source(bridge: &Bridge, source: &MatterSource) -> Result<(), HostError> {
    match source {
        MatterSource::NewFolder { folder } | MatterSource::ExistingFolder { folder } => {
            picked(bridge, *folder)
        }
        // The host's own default location: no folder ref, nothing to check.
        MatterSource::DefaultLocation => Ok(()),
    }
}

/// Whether this run's `pick_folder` issued `folder`: `command.invalid` when it did not.
fn picked(bridge: &Bridge, folder: FolderRefId) -> Result<(), HostError> {
    if lock(&bridge.issued).contains(&folder) {
        Ok(())
    } else {
        Err(invalid(format!(
            "folder ref {folder} was not picked in this run"
        )))
    }
}
