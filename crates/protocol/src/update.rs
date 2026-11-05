//! The updater as the windows read and drive it (decision record 0037): an [`UpdateView`]
//! answers `HostQuery::GetUpdate` and rides `HostEvent::UpdateChanged`; `HostCommand`'s
//! `CheckForUpdate`, `InstallUpdate` (with an [`InstallWhen`]), `CancelScheduledUpdate` and
//! `SetUpdateMode` (with an [`UpdateMode`]) drive it. A command in a state that does not take
//! it is refused with `command.invalid`; a failed check, download or install is
//! [`UpdateState::Failed`], and the code a window shows for it is `update.failed`.
//!
//! Versions are the release's own text (`0.4.1`), as `latest.json` and the app's
//! `tauri.conf.json` write them; this crate does not parse them.

use serde::{Deserialize, Serialize};

use crate::{Count, Timestamp};

/// What `GetUpdate` answers and `UpdateChanged` carries: the running version and where the
/// updater stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UpdateView {
    /// The version running now.
    pub current: String,
    /// Where the updater stands.
    pub state: UpdateState,
    /// `update.mode` in force: the lawyer's, or policy's over it.
    pub mode: UpdateMode,
    /// When the last check that did not fail ended; missing before the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<Timestamp>,
    /// The version this run updated from, when this is the first start after an install;
    /// missing otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_from: Option<String>,
}

/// Where the updater stands. JSON: `"idle"`, or a one-key object such as
/// `{"ready":{"version":"0.4.1"}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum UpdateState {
    /// Nothing to do until the next check.
    Idle,
    /// Asking the release endpoint whether a newer version exists.
    Checking,
    /// Downloading a newer version.
    Downloading {
        /// The version being downloaded.
        version: String,
        /// Bytes received so far.
        received: Count,
        /// The download's size, when the server said it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total: Option<Count>,
    },
    /// Downloaded and its signature checked: it installs on `InstallUpdate`, and in `auto` mode
    /// also at the next start.
    Ready {
        /// The version ready to install.
        version: String,
    },
    /// `InstallUpdate { when: after_turn }` while a turn runs: it installs once no thread has
    /// a running turn. `CancelScheduledUpdate` takes it back to `Ready`.
    Scheduled {
        /// The version waiting to install.
        version: String,
    },
    /// Shutting down to install; the app quits and starts again as the new version.
    Installing {
        /// The version installing.
        version: String,
    },
    /// The last check, download or install failed; the next check (by timer or
    /// `CheckForUpdate`) tries again.
    Failed {
        /// The version that failed, when one was known (missing for a failed check).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        /// When it failed.
        at: Timestamp,
        /// What failed.
        stage: FailedStage,
    },
    /// Clauseo does not update itself here; someone else does, and checks never run.
    Managed {
        /// Who manages updates.
        by: ManagedBy,
    },
}

/// What failed in an [`UpdateState::Failed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum FailedStage {
    /// Asking for the latest version.
    Check,
    /// Downloading it, or checking its signature.
    Download,
    /// Installing it, found at the next start.
    Install,
}

impl FailedStage {
    /// Every value, in declaration order.
    pub const ALL: [Self; 3] = [Self::Check, Self::Download, Self::Install];
}

/// Who manages updates in an [`UpdateState::Managed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ManagedBy {
    /// The firm's policy sets `update.mode` to `none`, or a policy source cannot be read.
    Policy,
    /// A per-machine install (Program Files on Windows): IT updates it.
    PerMachine,
    /// The app's folder is not writable by the lawyer (macOS), so an update would ask for an
    /// administrator.
    ReadOnlyLocation,
    /// A package manager updates it (Linux), or this build has no updater.
    Package,
}

impl ManagedBy {
    /// Every value, in declaration order.
    pub const ALL: [Self; 4] = [
        Self::Policy,
        Self::PerMachine,
        Self::ReadOnlyLocation,
        Self::Package,
    ];
}

/// `update.mode`: how updates install. A window sets only `auto` and `notify`; `none` comes
/// from policy (or a hand-edited settings file), and `SetUpdateMode { mode: none }` is refused
/// as `command.invalid`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum UpdateMode {
    /// Download in the background and install at the next start, or sooner on
    /// `InstallUpdate`.
    #[default]
    Auto,
    /// Download in the background, install only on `InstallUpdate`.
    Notify,
    /// Never check: updates are managed.
    None,
}

impl UpdateMode {
    /// Every value, in declaration order.
    pub const ALL: [Self; 3] = [Self::Auto, Self::Notify, Self::None];
}

/// When `InstallUpdate` installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum InstallWhen {
    /// Now: running turns are stopped as Stop stops them, then the app quits to install.
    Now,
    /// Once no thread has a running turn; at once when none runs.
    AfterTurn,
}

impl InstallWhen {
    /// Every value, in declaration order.
    pub const ALL: [Self; 2] = [Self::Now, Self::AfterTurn];
}
