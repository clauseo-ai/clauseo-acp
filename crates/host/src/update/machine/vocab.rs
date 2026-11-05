//! The machine's vocabulary: what start-up hands it ([`Start`]), what happens to it
//! ([`Input`]), what it asks the actor to do ([`Effect`]) and why it refuses a command
//! ([`Refusal`]). R23 PR 4's actor and PR 5's desktop adapter both live with these shapes.

use clauseo_counts::schema::UpdateResult;
use clauseo_protocol::{InstallWhen, Timestamp, UpdateMode};

use super::Jitter;
use crate::update::{
    CachedDownload, Downloaded, InstallKind, Offer, Progress, UpdateMarker, UpdaterError,
};

/// `update.mode` in force and whether policy locks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModeInForce {
    /// The lawyer's value, or policy's over it (`none` when a policy source cannot be read).
    pub(crate) mode: UpdateMode,
    /// Policy sets the key, so `ModeSet` is refused with [`Refusal::Locked`].
    pub(crate) locked: bool,
}

/// What start-up read, for [`Machine::start`].
#[derive(Debug, Clone)]
pub(crate) struct Start {
    /// The running version.
    pub(crate) current: String,
    /// [`Updater::install_kind`](super::Updater::install_kind).
    pub(crate) kind: InstallKind,
    /// `update.mode` in force.
    pub(crate) mode: ModeInForce,
    /// `update.json`, when there is one.
    pub(crate) marker: Option<UpdateMarker>,
    /// Last run's download, when one is on disk.
    pub(crate) cached: Option<CachedDownload>,
    /// The seeded source of the check delays.
    pub(crate) jitter: Jitter,
}

/// One thing that happened, for [`step`].
#[derive(Debug, Clone)]
pub(crate) enum Input {
    /// The timer of the last [`Effect::ArmTimer`] fired.
    TimerFired,
    /// `CheckForUpdate`.
    CheckRequested,
    /// The check of the last [`Effect::StartCheck`] ended.
    CheckDone(Result<Option<Offer>, UpdaterError>),
    /// The download posted its progress.
    Progress(Progress),
    /// The download of the last [`Effect::StartDownload`] ended.
    DownloadDone(Result<Downloaded, UpdaterError>),
    /// `InstallUpdate { when }`.
    InstallRequested(InstallWhen),
    /// `CancelScheduledUpdate`.
    CancelScheduled,
    /// `SetUpdateMode { mode }`.
    ModeSet(UpdateMode),
    /// The settings changed `update.mode` in force or its lock: a `SetUpdateMode` write (the
    /// actor checks it with `ModeSet` first, then hands over what the settings hold after it),
    /// or a policy reload.
    PolicyChanged(ModeInForce),
    /// Whether any thread has a running turn, sent each time that flips.
    TurnsRunning(bool),
    /// Every turn [`Effect::CancelTurns`] cancelled has ended.
    TurnsCancelled,
    /// The write [`Effect::WriteMarker`] asked for ended: `true` when `update.json` is on disk.
    MarkerWritten(bool),
}

/// What the actor does after a step, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Effect {
    /// Call [`Updater::check`](super::Updater::check); its result comes back as
    /// [`Input::CheckDone`].
    StartCheck,
    /// Call [`Updater::download`](super::Updater::download); its result comes back as
    /// [`Input::DownloadDone`].
    StartDownload(Offer),
    /// Drop the running download's future.
    CancelDownload,
    /// Fire [`Input::TimerFired`] at `at`, replacing any timer armed before.
    ArmTimer {
        /// When.
        at: Timestamp,
    },
    /// Stop every running turn as Stop does; send [`Input::TurnsCancelled`] once each has
    /// ended.
    CancelTurns,
    /// Hold new sends queued (`true`) or let them run again (`false`).
    GateNewTurns(bool),
    /// Write `update.json`, off the actor; send [`Input::MarkerWritten`] once it is done.
    WriteMarker(UpdateMarker),
    /// Delete `update.json`, off the actor: start-up has read it.
    ClearMarker,
    /// Shut down and exit with `Exit::Install`, handing desktop the download. Only once the
    /// marker is on disk.
    BeginInstall(Downloaded),
    /// Send `UpdateChanged` with [`Machine::view`].
    Publish,
    /// Count `update { result }`.
    Count(UpdateResult),
}

/// Why a command was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The state does not take it: `command.invalid`.
    NotNow,
    /// Policy locks `update.mode`: `settings.locked`.
    Locked,
}
