//! The install, once the host has stopped (decision record 0037, "The install, in order"). The
//! stopper thread (`lib.rs`) runs, in order: the host's stop, the log's flush, the counts' save,
//! then [`after_host`], which with `HostExit::Install`:
//!
//! 1. saves the window state (an install or a restart ends the app without the exit event
//!    window-state saves on);
//! 2. calls the crash guard's clean exit, a no-op until S22 ([`crash_clean_exit`]);
//! 3. flushes the log again, for the lines since;
//! 4. installs: the plugin's `install` verifies the bytes again; Windows exits inside it, macOS
//!    swaps the `.app` and returns;
//! 5. restarts the app, after an install that worked (macOS) or failed. A failed one leaves
//!    `update.json` behind with the version unchanged, so the next start shows `failed`.
//!
//! A download no plugin made (the `e2e` build's fake) is never installed: the app just exits.

use clauseo_host::{Downloaded, HostExit, UpdaterError};
#[cfg(not(target_os = "linux"))]
use tauri::{AppHandle, Runtime};

/// What the app does once [`after_host`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum After {
    /// Exits as it would have: nothing was installed.
    Exit,
    /// Restarts: an install ran, and worked or failed.
    Restart,
}

/// The steps of the install, apart from the decision, so tests run it without an app.
pub(super) trait Steps {
    /// Whether `downloaded` is one this app can install (the plugin's).
    fn installable(&self, downloaded: &Downloaded) -> bool;
    /// Saves the window state.
    fn save_window_state(&mut self);
    /// S22's crash guard's clean exit.
    fn crash_clean_exit(&mut self);
    /// Flushes the host log.
    fn flush_log(&mut self);
    /// The plugin's install. Windows never returns from it. Its error holds a fixed line, never
    /// a path or a URL: it is logged.
    fn install(&mut self, downloaded: &Downloaded) -> Result<(), UpdaterError>;
}

/// Runs the install when the host stopped for one (`ended`), in the module's order, and says
/// what the app does next.
pub(super) fn after_host(ended: HostExit, steps: &mut impl Steps) -> After {
    let HostExit::Install(downloaded) = ended else {
        return After::Exit;
    };
    let version = downloaded.version.as_str();
    if !steps.installable(&downloaded) {
        tracing::info!(
            target: LIFECYCLE,
            version,
            "the host stopped for an update this build does not install"
        );
        return After::Exit;
    }
    steps.save_window_state();
    steps.crash_clean_exit();
    tracing::info!(target: LIFECYCLE, version, "installing the update");
    steps.flush_log();
    match steps.install(&downloaded) {
        Ok(()) => tracing::info!(
            target: LIFECYCLE,
            version,
            "the update is installed; restarting"
        ),
        Err(error) => tracing::warn!(
            kind = kind(&error),
            what = line(&error),
            version,
            "the update did not install; restarting as the old version"
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

/// The real steps, over the app.
#[cfg(not(target_os = "linux"))]
pub(super) struct AppSteps<'a, R: Runtime>(&'a AppHandle<R>);

#[cfg(not(target_os = "linux"))]
impl<'a, R: Runtime> AppSteps<'a, R> {
    /// The steps over `app`.
    pub(super) fn new(app: &'a AppHandle<R>) -> Self {
        Self(app)
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

    fn install(&mut self, downloaded: &Downloaded) -> Result<(), UpdaterError> {
        super::plugin::install(self.0, downloaded)
    }
}
