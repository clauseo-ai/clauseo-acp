//! The host's log file (decision record 0047), started at the app's start and flushed when the
//! host stops. [`install`] is the one place the app starts it. A log that cannot open never
//! stops the app: the [`HostLog`] then holds nothing. The guard lives in Tauri's managed state,
//! so it lasts as long as the app and the stop thread can reach it ([`flush`]). Nothing here
//! logs a path.

use clauseo_host::log_file::LogGuard;
use clauseo_settings::{Env, Locations, PlatformDirs};
use tauri::{AppHandle, Manager as _, Runtime};

#[cfg(test)]
mod tests;

/// The log's guard in managed state. `LogGuard` is `Send` and `Sync` (its state is an `Arc` of
/// atomics and a mutex), so it needs no lock of its own.
pub(crate) struct HostLog(Option<LogGuard>);

/// Starts the host's log file under the data folder that `dirs` and `env` resolve to. The
/// guard holds nothing when the folders do not resolve or the file does not open.
pub(crate) fn install(dirs: &dyn PlatformDirs, env: &dyn Env) -> HostLog {
    let guard = Locations::resolve(dirs, env)
        .map(|locations| clauseo_host::log_file::install(&locations))
        .ok();
    HostLog(guard)
}

impl HostLog {
    /// Waits, at most the guard's own limit, for the lines queued so far to be written. Does
    /// nothing when no log started.
    pub(crate) fn flush(&self) {
        if let Some(guard) = self.0.as_ref() {
            guard.flush();
        }
    }
}

/// [`HostLog::flush`] for the guard in `app`'s managed state; nothing when there is none.
pub(crate) fn flush_app<R: Runtime>(app: &AppHandle<R>) {
    if let Some(log) = app.try_state::<HostLog>() {
        log.flush();
    }
}
