//! Whether the last run quit through the app's own quit, for the notice after Clauseo closed
//! unexpectedly (`HostQuery::LastExit`, the room's `crash-notice.tsx`).
//!
//! - [`begin`], at the top of setup: takes the OS lock on `last-run.lock` in the data folder and
//!   holds it for the life of the process. The OS lets go of it however the process ends, a
//!   crash and a kill included. A lock already held means another Clauseo runs on this data
//!   folder (a second start the single-instance plugin did not end: on Linux without a session
//!   bus it starts normally), and that start reads and writes nothing here. A held lock is
//!   tried for 2 s first: an update's restart, or a Windows that lets go of a dead process's lock
//!   late, is not another Clauseo. Then it reads
//!   `last-run`: `running` there means the last run never reached its quit. Then it writes
//!   `running` over it, whole or not at all (`Files::write_atomic`: a temporary file, a sync and
//!   a rename), before the window or the host starts, so a crash during start-up counts too.
//! - [`closed`], from every quit once the host has stopped and the log is flushed: a closed
//!   window, the Quit menu, a quit signal, the updater's relaunch or install (both run after the
//!   stop's flush), macOS's quit, logout and shutdown, and Windows ending the session (all
//!   through `RunEvent::Exit`, also when nothing runs, but not when its wait for the host's stop
//!   runs out), and a second quit signal. It writes `closed`, trying again briefly when Windows
//!   refuses the rename, then lets go of the lock.
//!   The file is overwritten rather than removed: one write path, and a file that is always
//!   there says which way the last run went.
//!
//! Nothing here logs a path.

use std::fs::{File, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use clauseo_settings::Files;
use tauri::{AppHandle, Manager as _, Runtime};

/// The lock file, held while this run lives.
pub(crate) const LOCK_FILE: &str = "last-run.lock";

/// The mark: [`RUNNING`] from start until a quit writes [`CLOSED`].
pub(crate) const MARK_FILE: &str = "last-run";

const RUNNING: &[u8] = b"running\n";
const CLOSED: &[u8] = b"closed\n";

/// How long [`begin`] waits for a held lock to be let go before it decides another Clauseo owns
/// the folder: the run an update restarts from may still be ending, and Windows lets go of a
/// dead process's locks late. Tried every [`LOCK_PAUSE`].
const LOCK_TRIES: u32 = 20;
const LOCK_PAUSE: Duration = Duration::from_millis(100);

/// How often [`RunMark::close`] tries the write when the OS refuses it: on Windows an antivirus
/// or the indexer holding `last-run` open makes the rename fail for a moment.
const CLOSE_TRIES: u32 = 3;
const CLOSE_PAUSE: Duration = Duration::from_millis(50);

/// Waits: `std::thread::sleep` in the app, a hand-run step in tests.
pub(crate) type Pause<'a> = &'a mut dyn FnMut(Duration);

/// How the last run ended, as [`begin`] found it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LastRun {
    /// It quit through the app's quit, or there was none.
    Clean,
    /// It never reached its quit: a crash, a kill or a power cut.
    Unclean,
    /// Another Clauseo holds the data folder: this start touched nothing.
    OtherInstance,
}

/// This run's mark: the held lock, and where [`RunMark::close`] writes.
pub(crate) struct RunMark {
    /// Held until [`RunMark::close`] or the process ends; never read.
    _lock: Option<File>,
    mark: PathBuf,
}

impl RunMark {
    /// Writes [`CLOSED`]: this run quit through its quit. Then lets go of the lock, so a run
    /// started now (an update's restart) does not wait on it. A write the OS refuses is tried
    /// [`CLOSE_TRIES`] times, [`CLOSE_PAUSE`] apart, and logged after the last.
    pub(crate) fn close(self, files: &dyn Files, pause: Pause<'_>) {
        for attempt in 1..=CLOSE_TRIES {
            match files.write_atomic(&self.mark, CLOSED) {
                Ok(()) => return,
                Err(error)
                    if error.kind() == io::ErrorKind::PermissionDenied && attempt < CLOSE_TRIES =>
                {
                    pause(CLOSE_PAUSE);
                }
                Err(error) => {
                    tracing::warn!(kind = ?error.kind(), "the clean quit was not marked");
                    return;
                }
            }
        }
    }
}

/// How the last run ended and this run's mark, in managed state, for [`unclean`] and
/// [`closed`].
pub(crate) struct RunMarkState {
    last: LastRun,
    mark: Mutex<Option<RunMark>>,
}

/// Reads how the last run ended from `dir` (the data folder) and marks this one running (see the
/// module's notes). A lock file that cannot be opened is logged, and the mark is used without a
/// lock; one that will not write is logged, and this run is not marked.
pub(crate) fn begin(dir: &Path, files: &dyn Files, pause: Pause<'_>) -> (LastRun, Option<RunMark>) {
    let lock = match open_lock(&dir.join(LOCK_FILE)) {
        Ok(file) => match take_lock(&file, pause) {
            Ok(true) => Some(file),
            Ok(false) => return (LastRun::OtherInstance, None),
            Err(error) => {
                tracing::warn!(kind = ?error.kind(), "the last run's lock was not taken");
                None
            }
        },
        Err(error) => {
            tracing::warn!(kind = ?error.kind(), "the last run's lock did not open");
            None
        }
    };
    let mark = dir.join(MARK_FILE);
    let last = match files.read(&mark) {
        Ok(Some(bytes)) if bytes.starts_with(b"running") => LastRun::Unclean,
        Ok(_) => LastRun::Clean,
        Err(error) => {
            tracing::warn!(kind = ?error.kind(), "the last run's mark did not read");
            LastRun::Clean
        }
    };
    if let Err(error) = files.write_atomic(&mark, RUNNING) {
        tracing::warn!(kind = ?error.kind(), "this run was not marked running");
        return (last, None);
    }
    (last, Some(RunMark { _lock: lock, mark }))
}

/// Takes `file`'s lock, waiting up to [`LOCK_TRIES`] × [`LOCK_PAUSE`] while another process
/// holds it: `false` when it is held all that time.
fn take_lock(file: &File, pause: Pause<'_>) -> io::Result<bool> {
    for attempt in 1..=LOCK_TRIES {
        match file.try_lock() {
            Ok(()) => return Ok(true),
            Err(TryLockError::WouldBlock) if attempt < LOCK_TRIES => pause(LOCK_PAUSE),
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Error(error)) => return Err(error),
        }
    }
    Ok(false)
}

/// Setup's step: [`begin`] on the data folder `dir`, its outcome kept in `app`'s state.
pub(crate) fn start<R: Runtime>(app: &AppHandle<R>, dir: &Path) {
    let (last, mark) = begin(dir, &clauseo_settings::SystemFiles, &mut sleep);
    if last == LastRun::Unclean {
        tracing::info!(target: crate::startup::TARGET, "start-up: the last run did not quit");
    }
    app.manage(RunMarkState {
        last,
        mark: Mutex::new(mark),
    });
}

/// Whether the last run ended without its quit, for the host (`with_last_run_unclean`).
pub(crate) fn unclean<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<RunMarkState>()
        .is_some_and(|state| state.last == LastRun::Unclean)
}

/// The quit reached its end: marks this run closed (see the module's notes).
pub(crate) fn closed<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<RunMarkState>() else {
        return;
    };
    // Taken out: the lock goes with it, and a later quit path finds nothing to do.
    let mark = state
        .mark
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(mark) = mark {
        mark.close(&clauseo_settings::SystemFiles, &mut sleep);
    }
}

/// The app's [`Pause`]: at most 2 s at a contested start, 100 ms at a refused quit write.
#[expect(
    clippy::disallowed_methods,
    reason = "the last run's lock and mark wait briefly on the OS; tests pass their own pause"
)]
fn sleep(duration: Duration) {
    std::thread::sleep(duration);
}

/// Opens `path` for reading and writing, creating it and its folder when missing. Never
/// truncated and never renamed, so every start locks the same file.
#[expect(
    clippy::disallowed_methods,
    reason = "the last run's lock file: created in place once, never replaced, so its OS lock holds"
)]
fn open_lock(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

#[cfg(test)]
mod tests;
