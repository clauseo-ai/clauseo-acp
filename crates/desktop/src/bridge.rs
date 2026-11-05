//! The bridge between the webview's commands and the session host. It holds the host's
//! [`HostCell`] (every call waits there for the host's start), each window's channel and
//! stream state (`window.rs`, driven by `pump.rs`), and the folder refs this run's
//! `pick_folder` issued. Every host call runs as `Principal::LocalDesktop`; the webview never
//! names a principal.
//!
//! A window's entry lives from its page's `open_stream` until the window is destroyed
//! (`close_window`, from `settings_window::on_destroyed`): then its channel, pumps, app pump and
//! watchdog go. What a window may dispatch or query depends on its label (`admit.rs`, `query`).

use std::collections::{HashMap, HashSet};
use std::path::{MAIN_SEPARATOR, Path};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use clauseo_host::HostHandle;
use clauseo_protocol::{
    Ack, AgentChoiceId, CommandEnvelope, CommandId, CommandOutcome, ErrorCode, FolderRefId,
    HostCommand, HostError, HostQuery, Language, PermissionEntry, PermissionRequestId, Principal,
    QueryResult, RefreshWhy, StreamId, ThreadId, ViewPosition,
};
use tauri::ipc::Channel;

use crate::dialogs::{Confirm, Picker};
use crate::frame::Frame;
use crate::host_start::HostCell;
use crate::picked::{self, PickedFolder};
use crate::settings_window::SETTINGS_WINDOW;

mod admit;
mod permission;
mod pump;
mod search;
mod throttle;
mod window;

use pump::Window;
use throttle::RefreshThrottle;
use window::{AckError, Feed};

// The tests (`tests.rs`, `tests/permission.rs`) name these through this module.
#[cfg(test)]
use clauseo_protocol::{MatterSource, ViewEntry};
#[cfg(test)]
use permission::needs_confirm;

/// The principal of every call: the desktop app on this computer.
const PRINCIPAL: Principal = Principal::LocalDesktop;

/// The option ids claude-agent-acp 0.89.1 offers when it leaves plan mode that switch the
/// session to `bypassPermissions` (`exit-plan-bypass`, `exit-plan-clear-bypass`) or to auto
/// mode (`exit-plan-auto`, `exit-plan-clear-auto`). Choosing one is a mode change, refused for
/// the reason `SetMode` is (see `dispatch`). The host's session meta sends
/// `allowDangerouslySkipPermissions: false`, so the bypass options are no longer offered; the
/// refusal stays for an adapter that offers them anyway, and for the auto options.
const MODE_OPTIONS: [&str; 4] = [
    "exit-plan-bypass",
    "exit-plan-clear-bypass",
    "exit-plan-auto",
    "exit-plan-clear-auto",
];

/// A window's stream, with the number of the `open_stream` call that made it.
type Numbered = (u64, Arc<Mutex<Window>>);

/// What the commands share: the host, the windows and the folder refs issued.
pub(crate) struct Bridge {
    /// Where the host's start lands.
    cell: Arc<HostCell>,
    /// Each window's stream, by webview label.
    windows: Mutex<HashMap<String, Numbered>>,
    /// The number the next `open_stream` call or window build takes, in the order they arrive.
    opens: AtomicU64,
    /// The number of each label's latest successful build (`window_built`): a window destroyed
    /// after a newer one of its label was built leaves that one's stream alone (`close_window`).
    builds: Mutex<HashMap<String, u64>>,
    /// Per label, the number below which every `open_stream` belongs to a destroyed window
    /// (`close_window`): a call still waiting for the host when its window closed drops its
    /// stream instead of keeping it. Locked only inside `windows`' lock.
    closed: Mutex<HashMap<String, u64>>,
    /// The folder refs `pick_folder` issued in this run; `CreateMatter` takes no other.
    issued: Mutex<HashSet<FolderRefId>>,
    /// The settings window's `RefreshLimits`, at most one per agent per 10 s.
    refreshes: RefreshThrottle,
}

/// Locks `mutex`, taking the data of one a panicking holder left: every critical section here
/// leaves the state whole between statements.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A `command.invalid` failure with `message`.
fn invalid(message: impl Into<String>) -> HostError {
    HostError::new(ErrorCode::CommandInvalid, message)
}

impl Bridge {
    /// A bridge to the host whose start lands in `cell`, with no window yet.
    pub(crate) fn new(cell: Arc<HostCell>) -> Self {
        Self {
            cell,
            windows: Mutex::new(HashMap::new()),
            opens: AtomicU64::new(0),
            builds: Mutex::new(HashMap::new()),
            closed: Mutex::new(HashMap::new()),
            issued: Mutex::new(HashSet::new()),
            refreshes: RefreshThrottle::default(),
        }
    }

    /// The host's cell, for exit.
    pub(crate) fn cell(&self) -> Arc<HostCell> {
        Arc::clone(&self.cell)
    }

    /// The host, once its start finished: every call waits here, and after a failed start
    /// gets that start's error.
    pub(crate) async fn host(&self) -> Result<&HostHandle, HostError> {
        self.cell.host().await
    }

    /// The app's language (`ui.language`), for the words Rust draws: English when the settings
    /// do not read or the host did not start.
    pub(crate) async fn language(&self) -> Language {
        let Ok(host) = self.host().await else {
            return Language::En;
        };
        match host.query(PRINCIPAL, HostQuery::GetSettings).await {
            Ok(QueryResult::Settings(settings)) => settings.ui.language,
            _ => Language::En,
        }
    }

    /// Checks `command`'s shape and that the window `label` may send it (`admit`), and runs it.
    pub(crate) async fn dispatch(
        &self,
        label: &str,
        command: CommandEnvelope,
    ) -> Result<CommandOutcome, HostError> {
        self.dispatch_at(label, command, pump::now()).await
    }

    /// `dispatch` at `now`. A `RefreshLimits` from the settings window that its throttle holds
    /// back (`throttle.rs`) answers `Done` and reaches no host.
    async fn dispatch_at(
        &self,
        label: &str,
        command: CommandEnvelope,
        now: Instant,
    ) -> Result<CommandOutcome, HostError> {
        let checked = command.validate().map_err(HostError::invalid_command)?;
        self.admit(label, checked.command())?;
        if label == SETTINGS_WINDOW
            && let HostCommand::RefreshLimits { agent, .. } = checked.command()
            && !self.refreshes.pass(*agent, now)
        {
            return Ok(CommandOutcome::Done);
        }
        self.host().await?.dispatch(checked.into_envelope()).await
    }

    /// A window got focus: `RefreshLimits { agent: None, why: Shown }`, from Rust. The host
    /// reads only limits older than 5 minutes, so the bridge does not throttle it.
    pub(crate) async fn shown(&self) -> Result<CommandOutcome, HostError> {
        let shown = HostCommand::RefreshLimits {
            agent: None,
            why: RefreshWhy::Shown,
        };
        self.run(shown).await
    }

    /// Runs `command`, which the bridge itself makes, under a new command id.
    /// Whether the window `label` may send `command` (`admit.rs`).
    fn admit(&self, label: &str, command: &HostCommand) -> Result<(), HostError> {
        admit::admit(self, label, command)
    }

    async fn run(&self, command: HostCommand) -> Result<CommandOutcome, HostError> {
        let command_id =
            CommandId::new(uuid::Uuid::now_v7()).map_err(HostError::invalid_command)?;
        let envelope = CommandEnvelope {
            command_id,
            command,
        };
        let checked = envelope.validate().map_err(HostError::invalid_command)?;
        self.host().await?.dispatch(checked.into_envelope()).await
    }

    /// Answers the window `label`'s `query`. `main` may send every read the host has: the
    /// match is an allow-list like `admit_main`'s, so a read the host adds does not compile here
    /// until the bridge rules on it. The `settings` window reads the settings (0028) and the
    /// updater's state (0037) alone, and any other window nothing.
    pub(crate) async fn query(
        &self,
        label: &str,
        query: HostQuery,
    ) -> Result<QueryResult, HostError> {
        match label {
            crate::MAIN_WINDOW => {}
            SETTINGS_WINDOW if matches!(query, HostQuery::GetSettings | HostQuery::GetUpdate) => {}
            SETTINGS_WINDOW => {
                return Err(invalid(
                    "the settings window reads only the settings and the update state",
                ));
            }
            _ => return Err(invalid(format!("window {label} may send no query"))),
        }
        match &query {
            HostQuery::ListMatters
            | HostQuery::GetDefaultLocation
            | HostQuery::ListThreads { .. }
            | HostQuery::PageEntries { .. }
            | HostQuery::CompareVersions { .. }
            | HostQuery::PreviewRestore { .. }
            // Undo's "changed after Clauseo's edit" ask (#360): reads the folder, writes nothing.
            | HostQuery::PreviewUndo { .. }
            | HostQuery::ReadBlob { .. }
            | HostQuery::GetSettings
            // The updater's state (0037): both windows show it.
            | HostQuery::GetUpdate
            | HostQuery::ListAgents
            // H8's wall (#131): reads the folder's stats, and names files by refs.
            | HostQuery::ListPapers { .. } => {}
            HostQuery::Search(request) => return search::page(self, label, request.clone()).await,
            // The New matter dialog's folder warning: only a ref this run's picker issued, as
            // for `CreateMatter`.
            HostQuery::FolderSupport { source } => admit::picked_source(self, source)?,
        }
        self.host().await?.query(PRINCIPAL, query).await
    }

    /// The window `label`'s stream, or `command.invalid` before its `open_stream`.
    fn window(&self, label: &str) -> Result<Arc<Mutex<Window>>, HostError> {
        lock(&self.windows)
            .get(label)
            .map(|(_, window)| Arc::clone(window))
            .ok_or_else(|| invalid(format!("window {label} has not opened its stream")))
    }

    /// Makes `channel` the window's one channel, and follows the limits on it: the host sends
    /// the current view first, then each change. A window that had one (a page reload) drops
    /// it and every subscription it had. Calls that wait together for the host's start (a
    /// reload while it starts) wake in any order: the one that arrived last wins, so a dead
    /// page's channel never replaces the new page's. A call that arrived before its window was
    /// destroyed (`close_window`) drops its stream when it wakes.
    pub(crate) async fn open_stream(
        &self,
        label: &str,
        channel: Channel<Frame>,
    ) -> Result<(), HostError> {
        // Numbered before the wait: the order the calls arrived in, not the order they wake.
        let number = self.opens.fetch_add(1, Ordering::Relaxed);
        let host = self.host().await?;
        let window = Window::open(host.clone(), channel, pump::AppEvents::of(label));
        let mut windows = lock(&self.windows);
        let newer = windows.get(label).is_some_and(|(kept, _)| *kept > number);
        let dead = lock(&self.closed)
            .get(label)
            .is_some_and(|below| number < *below);
        // A later call's window is in place, or this call's window was destroyed while it
        // waited: this one, a dead page's, is dropped instead.
        let old = if newer || dead {
            Some(window)
        } else {
            // Only the kept window follows the limits: a dropped one's channel is dead. `main`
            // and the settings window (its Plan and usage, U12) show them; no other label does.
            if crate::follows_limits(label) {
                pump::follow_limits(host, &window);
            }
            windows
                .insert(label.to_owned(), (number, window))
                .map(|(_, old)| old)
        };
        drop(windows);
        drop(old);
        Ok(())
    }

    /// Numbers a window's build, before it is built: every `open_stream` its page sends gets a
    /// higher number. Recorded with `window_built` once the build worked.
    pub(crate) fn number_build(&self) -> u64 {
        self.opens.fetch_add(1, Ordering::Relaxed)
    }

    /// Records that the window `label` was built as `number` (`number_build`). Only a build
    /// that worked is recorded: a second open that fails on the existing label must not make
    /// the first window's close look late. The number goes to `close_window` when that window
    /// is destroyed.
    pub(crate) fn window_built(&self, label: &str, number: u64) {
        let mut builds = lock(&self.builds);
        let latest = builds.entry(label.to_owned()).or_insert(number);
        *latest = (*latest).max(number);
    }

    /// For tests: the number of the window `label`'s latest recorded build.
    #[cfg(test)]
    pub(crate) fn latest_build(&self, label: &str) -> Option<u64> {
        lock(&self.builds).get(label).copied()
    }

    /// Drops the stream of the window `label` built as number `built`, which was destroyed: its
    /// channel, every pump, the app pump and the watchdog, so their host subscriptions go. When a
    /// newer window of that label was built since, only a stream opened before that build goes:
    /// a late close never removes the newer page's stream. Tauri removes a destroyed window
    /// before its handlers run and builds a window on the event loop only after them, so on Wry
    /// the guard never fires; it holds whatever order a runtime keeps. An `open_stream` of the
    /// destroyed window still waiting for the host drops its stream when it wakes (`closed`).
    pub(crate) fn close_window(&self, label: &str, built: u64) {
        let newer = {
            let mut builds = lock(&self.builds);
            match builds.get(label).copied() {
                Some(latest) if latest > built => Some(latest),
                _ => {
                    builds.remove(label);
                    None
                }
            }
        };
        // Every stream numbered below this is the destroyed window's: before the newer build,
        // or with none, every one numbered so far.
        let below = newer.unwrap_or_else(|| self.opens.fetch_add(1, Ordering::Relaxed));
        let mut windows = lock(&self.windows);
        let mut closed = lock(&self.closed);
        let tombstone = closed.entry(label.to_owned()).or_insert(below);
        *tombstone = (*tombstone).max(below);
        drop(closed);
        let stale = windows
            .get(label)
            .is_some_and(|(opened, _)| *opened < below);
        let old = if stale { windows.remove(label) } else { None };
        drop(windows);
        // Dropped outside the lock: aborting the pumps takes the window's own lock.
        drop(old);
    }

    /// Follows `stream` on the window's channel from `after`.
    pub(crate) async fn subscribe(
        &self,
        label: &str,
        stream: StreamId,
        after: Option<ViewPosition>,
    ) -> Result<(), HostError> {
        let window = self.window(label)?;
        // Before `follow`: a stream followed with no host would never be forgotten.
        let host = self.host().await?;
        let after = after.map(|position| (position.epoch, position.cursor));
        let restart = pump::follow(&window, stream, after);
        pump::start(host, &window, restart)
            .await
            .inspect_err(|_| pump::forget(&window, restart))
    }

    /// Follows the app-wide events on the window's channel (`pump::follow_feed`).
    pub(crate) async fn subscribe_app(&self, label: &str) -> Result<(), HostError> {
        let window = self.window(label)?;
        let host = self.host().await?;
        let restart = pump::follow_feed(&window, Feed::App);
        pump::start_feed(host, &window, Feed::App, restart, false)
            .await
            .inspect_err(|_| pump::forget_feed(&window, Feed::App, restart))
    }

    /// Stops following `stream` on the window's channel.
    pub(crate) fn unsubscribe(&self, label: &str, stream: StreamId) -> Result<(), HostError> {
        let window = self.window(label)?;
        pump::unfollow(&window, stream);
        Ok(())
    }

    /// Answers a permission request with `option`, which it must have offered (`permission.rs`).
    pub(crate) async fn answer_permission(
        &self,
        confirm: &dyn Confirm,
        thread: ThreadId,
        request: PermissionRequestId,
        option: AgentChoiceId,
    ) -> Result<(), HostError> {
        permission::answer(self, confirm, thread, request, option).await
    }

    /// The permission entry `request` of `thread`'s view, when it is pending (`permission.rs`).
    async fn pending_permission(
        &self,
        thread: ThreadId,
        request: PermissionRequestId,
    ) -> Result<PermissionEntry, HostError> {
        permission::pending(self, thread, request).await
    }

    /// Records the window's ack, `messages` channel messages processed since `open_stream` and
    /// the stream positions in `positions` (maybe none), and passes each position of a stream
    /// the window follows on to the host. An ack naming a stream twice, or more messages than
    /// were sent, is refused whole.
    pub(crate) async fn ack(
        &self,
        label: &str,
        positions: Vec<Ack>,
        messages: u32,
    ) -> Result<(), HostError> {
        let window = self.window(label)?;
        let followed = pump::ack(&window, &positions, messages).map_err(|error| match error {
            AckError::TooManyMessages => invalid(format!(
                "ack of {messages} messages is more than the window was sent"
            )),
            AckError::RepeatedStream(stream) => {
                invalid(format!("ack names stream {stream} more than once"))
            }
        })?;
        let host = self.host().await?;
        for position in followed {
            host.ack(PRINCIPAL, position.stream, position.epoch, position.cursor)
                .await?;
        }
        Ok(())
    }

    /// Asks `picker` for a folder and registers it, with the display `picked::display` gives
    /// it under `home`; `None` when the lawyer cancelled. Whether the folder may be a matter is
    /// the host's rule: `CreateMatter` refuses a refused folder with one `matter.folder_*` code
    /// per reason (decision record 0022).
    pub(crate) async fn pick_folder(
        &self,
        picker: &dyn Picker,
        home: Option<&Path>,
    ) -> Result<Option<PickedFolder>, HostError> {
        let Some(path) = picker.pick().await else {
            return Ok(None);
        };
        let display = picked::display(&path, home, MAIN_SEPARATOR);
        let folder = self.host().await?.register_folder(path).await?;
        lock(&self.issued).insert(folder);
        Ok(Some(PickedFolder { folder, display }))
    }
}

#[cfg(test)]
mod tests;
