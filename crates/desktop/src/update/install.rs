//! The install, once the host has stopped (decision record 0037, "The install, in order"). The
//! stopper thread (`lib.rs`) runs, in order: the host's stop, the log's flush, the counts' save,
//! then [`after_host`], which with `HostExit::Install`:
//!
//! 0. installs nothing when an agents job outlived its bound at quit (`agents_stopped` false: a
//!    child may still hold the app's files open); it saves the window state and restarts as the
//!    old version, so the next start shows `failed`;
//! 1. saves the window state (an install or a restart ends the app without the exit event
//!    window-state saves on);
//! 2. calls the crash guard's clean exit, a no-op until S22 ([`crash_clean_exit`]);
//! 3. flushes the log again, for the lines since;
//! 4. installs, on a thread of its own, watched ([`watch_install`]): the plugin's `install`
//!    verifies the bytes again; Windows exits inside it, macOS swaps the `.app` and returns. One
//!    that has not returned within [`Watch::INSTALL`] is given up on;
//! 5. restarts the app, after an install that worked (macOS), failed, or was given up on. A
//!    failed one leaves `update.json` behind with the version unchanged, so the next start shows
//!    `failed`.
//!
//! The watchdog's bound, and why it cannot cut a swap short, are on [`Watch::INSTALL`].
//!
//! A download no plugin made (the `e2e` build's fake) is never installed: the app just exits.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use clauseo_host::{Downloaded, HostExit, UpdaterError};
#[cfg(not(target_os = "linux"))]
use tauri::{AppHandle, Runtime};

/// What the app does once [`after_host`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum After {
    /// Exits as it would have: nothing was installed.
    Exit,
    /// Restarts: an install ran, and worked or failed.
    Restart,
}

/// The steps of the install, apart from the decision, so tests run it without an app.
pub(crate) trait Steps {
    /// Whether `downloaded` is one this app can install (the plugin's).
    fn installable(&self, downloaded: &Downloaded) -> bool;
    /// Saves the window state.
    fn save_window_state(&mut self);
    /// S22's crash guard's clean exit.
    fn crash_clean_exit(&mut self);
    /// Flushes the host log.
    fn flush_log(&mut self);
    /// Starts the plugin's install of `downloaded` on a thread of its own and returns where its
    /// answer arrives. Windows never answers: the process exits inside it. Its error holds a fixed
    /// line, never a path or a URL: it is logged.
    fn start_install(&mut self, downloaded: Downloaded) -> Receiver<Result<(), UpdaterError>>;
    /// Whether the app is where it was installed (its executable is there), read before giving
    /// up on a hung install: false only between the two moves of a swap the file system cannot
    /// make in one step.
    fn app_in_place(&self) -> bool;
}

/// How long [`after_host`] waits for the install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Watch {
    /// How long the install may take before it is given up on.
    pub(crate) bound: Duration,
    /// Past the bound, while the app is not in place (a swap between its two moves), how often
    /// to look again...
    pub(crate) poll: Duration,
    /// ...and how many times, before restarting all the same.
    pub(crate) polls: u32,
}

impl Watch {
    /// The install's watch: 5 minutes, then up to 60 more seconds while the app is not in place.
    ///
    /// The bound covers only what runs inside this process. Windows: verifying the bytes,
    /// writing the installer to a temporary file and starting it, then the exit; the NSIS
    /// installer (passive mode, however slow the disk) runs after the exit, in its own process,
    /// so no bound here can cut it short. macOS: verifying, unpacking the `.app` beside it (the
    /// slow part: seconds for a bundle of a few hundred MB, under a minute on a slow disk), then
    /// the swap, and the administrator's password prompt when the folder needs one. Five minutes
    /// is several times the slowest unpack and leaves a person time to answer the prompt.
    ///
    /// Why it cannot cut a swap short: the plugin swaps the bundles with one `renamex_np`
    /// (`RENAME_SWAP`, APFS), a single system call that a process exit cannot split. Only where
    /// the file system cannot swap (HFS+) does it move the old app out, then the new one in, two
    /// renames a few microseconds apart; between them the app is not in place, so the watchdog
    /// waits (`poll` × `polls`) for the install instead of restarting. Giving up anywhere
    /// else leaves the old app in place (litter in the temporary folder, which macOS clears) or
    /// the new one, and the next start's marker reads `failed` or `updated_from`. The one
    /// window left is the instant between the check and the restart; were the second move lost
    /// there, the old app sits in the temporary folder and the person reinstalls.
    pub(crate) const INSTALL: Self = Self {
        bound: Duration::from_secs(300),
        poll: Duration::from_secs(1),
        polls: 60,
    };
}

/// How a watched install ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Watched {
    /// It returned, with its answer.
    Returned(Result<(), UpdaterError>),
    /// Its thread ended without an answer (it panicked, or never started).
    Lost,
    /// It had not returned within the bound: the app restarts and leaves it.
    GaveUp,
}

/// Waits on `answer` for the install, as [`Watch`] says: up to `watch.bound`, then, while
/// `app_in_place` says no, `watch.polls` more times `watch.poll`.
pub(crate) fn watch_install(
    answer: &Receiver<Result<(), UpdaterError>>,
    watch: Watch,
    app_in_place: impl Fn() -> bool,
) -> Watched {
    let mut wait = watch.bound;
    let mut polls = 0;
    loop {
        match answer.recv_timeout(wait) {
            Ok(result) => return Watched::Returned(result),
            Err(RecvTimeoutError::Disconnected) => return Watched::Lost,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if polls >= watch.polls || app_in_place() {
            return Watched::GaveUp;
        }
        polls += 1;
        wait = watch.poll;
    }
}

/// Runs the install when the host stopped for one (`ended`), in the module's order, and says
/// what the app does next. `agents_stopped` is the quit's answer that every agents job ended
/// within its bound; without it nothing is installed.
pub(crate) fn after_host(
    ended: HostExit,
    agents_stopped: bool,
    steps: &mut impl Steps,
    watch: Watch,
) -> After {
    let HostExit::Install(downloaded) = ended else {
        return After::Exit;
    };
    let version = downloaded.version.clone();
    let version = version.as_str();
    if !steps.installable(&downloaded) {
        tracing::info!(
            target: LIFECYCLE,
            version,
            "the host stopped for an update this build does not install"
        );
        return After::Exit;
    }
    steps.save_window_state();
    if !agents_stopped {
        tracing::warn!(
            target: LIFECYCLE,
            kind = "agents",
            version,
            "an agents job was still running at quit: the update is not installed; restarting as \
             the old version"
        );
        steps.flush_log();
        return After::Restart;
    }
    steps.crash_clean_exit();
    tracing::info!(target: LIFECYCLE, version, "installing the update");
    steps.flush_log();
    let answer = steps.start_install(downloaded);
    match watch_install(&answer, watch, || steps.app_in_place()) {
        Watched::Returned(Ok(())) => tracing::info!(
            target: LIFECYCLE,
            version,
            "the update is installed; restarting"
        ),
        Watched::Returned(Err(error)) => tracing::warn!(
            target: LIFECYCLE,
            kind = kind(&error),
            what = line(&error),
            version,
            "the update did not install; restarting as the old version"
        ),
        Watched::Lost => tracing::warn!(
            target: LIFECYCLE,
            kind = "lost",
            version,
            "the update's install ended without an answer; restarting"
        ),
        Watched::GaveUp => tracing::warn!(
            target: LIFECYCLE,
            kind = "hung",
            version,
            bound_secs = watch.bound.as_secs(),
            "the update's install did not return in time; restarting"
        ),
    }
    steps.flush_log();
    After::Restart
}

/// The host log's target for the install's lines (0047's lifecycle targets: the file keeps info
/// from a `clauseo_*` crate's `::lifecycle` target), so it records that an install ran.
const LIFECYCLE: &str = "clauseo_desktop::lifecycle";

/// An error's fixed line, for the log. Every error [`Steps::install`] gives is one of
/// `plugin.rs`'s fixed lines (`failure`), never the plugin's text, so it names no path or URL.
fn line(error: &UpdaterError) -> &str {
    match error {
        UpdaterError::Network(line)
        | UpdaterError::Signature(line)
        | UpdaterError::Io(line)
        | UpdaterError::Other(line) => line,
    }
}

/// An error's kind, for the log.
fn kind(error: &UpdaterError) -> &'static str {
    match error {
        UpdaterError::Network(_) => "network",
        UpdaterError::Signature(_) => "signature",
        UpdaterError::Io(_) => "io",
        UpdaterError::Other(_) => "other",
    }
}

/// The seam for S22's `clauseo_crash::Guard::clean_exit`, which will delete
/// `<data_dir>/crash/running` and stop the crash handler's child, so the installer can replace
/// `Clauseo.exe` and the exit does not read as a crash. A no-op until S22 merges.
pub(super) fn crash_clean_exit() {}

/// The real steps, over the app, and the app's executable as it was before the install.
#[cfg(not(target_os = "linux"))]
pub(super) struct AppSteps<'a, R: Runtime>(&'a AppHandle<R>, Option<std::path::PathBuf>);

#[cfg(not(target_os = "linux"))]
impl<'a, R: Runtime> AppSteps<'a, R> {
    /// The steps over `app`.
    pub(super) fn new(app: &'a AppHandle<R>) -> Self {
        Self(app, std::env::current_exe().ok())
    }
}

#[cfg(not(target_os = "linux"))]
impl<R: Runtime> Steps for AppSteps<'_, R> {
    fn installable(&self, downloaded: &Downloaded) -> bool {
        downloaded
            .handle
            .downcast_ref::<super::plugin::Pending>()
            .is_some()
    }

    fn save_window_state(&mut self) {
        use tauri_plugin_window_state::AppHandleExt as _;
        if let Err(error) = self.0.save_window_state(crate::window_size::state_flags()) {
            // The plugin's error can name the config folder: only that it failed.
            drop(error);
            tracing::warn!("the window state was not saved before the update");
        }
    }

    fn crash_clean_exit(&mut self) {
        crash_clean_exit();
    }

    fn flush_log(&mut self) {
        crate::host_log::flush_app(self.0);
    }

    fn start_install(&mut self, downloaded: Downloaded) -> Receiver<Result<(), UpdaterError>> {
        let (answer, answered) = std::sync::mpsc::channel();
        let app = self.0.clone();
        // A thread that never starts drops `answer`: the watch reads it as lost.
        drop(
            std::thread::Builder::new()
                .name("clauseo-update-install".to_owned())
                .spawn(move || {
                    // The watch gave up and the app is restarting: nothing to tell.
                    let _unheard = answer.send(super::plugin::install(&app, &downloaded));
                }),
        );
        answered
    }

    fn app_in_place(&self) -> bool {
        // Unknown counts as in place: restarting then is no worse than waiting.
        self.1
            .as_deref()
            .is_none_or(|exe| exe.try_exists().unwrap_or(true))
    }
}
