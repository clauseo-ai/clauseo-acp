//! The run's own deadline (`--deadline <seconds>`, `args.rs`): the installer runs the setup exe
//! elevated and, per user, can't end it, so the exe bounds itself. The deadline is a hard
//! ceiling taken from the flag alone; nothing extends it. Without the flag nothing here runs.
//!
//! When it is armed (`exe.rs`, through `deadline_os::arm`):
//!
//! - the process goes into a job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and no breakaway, so
//!   nothing it might start outlives it (it starts nothing today; belt and braces);
//! - a watchdog thread ([`start`]) sets the cancel flag ([`cancelled`]) at `D - R`, `R` being
//!   [`ROLLBACK_BOUND`] (at once when `D <= R`). The main flow checks the flag before each
//!   change it makes (`create::checkpoint`, `remove.rs`, `adopt.rs`); once it is set, the main
//!   flow stops there, runs its own rollback (`rollback.rs`) and exits
//!   [`SetupExit::DeadlineReached`](super::SetupExit::DeadlineReached);
//! - at `D` the watchdog ends the process regardless (`TerminateProcess` on itself) with that
//!   code. The watchdog never undoes anything itself, so nothing is undone while a stuck main
//!   thread might still be changing it. What a run ended this way leaves, its intent and its
//!   record name for the next set-up or `--remove` (`intent.rs`).
//!
//! The watchdog logs the kind of each event only, never a path or a name. The clock and the
//! two actions are parameters, so its timing is tested on every OS without waiting.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// `R`: how long before the deadline the cancel flag is set, which is what the main flow has
/// left for its rollback.
pub const ROLLBACK_BOUND: Duration = Duration::from_secs(15);

/// The process's cancel flag: set once, by the watchdog, never cleared.
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Whether this process's deadline is near: the main flow makes no further change, but rolls
/// back and exits. Always `false` in a run with no `--deadline`.
pub fn cancelled() -> bool {
    CANCEL.load(Ordering::SeqCst)
}

/// Sets this process's cancel flag (the watchdog's first action in the real exe).
#[cfg(windows)]
pub(super) fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// How a [`Clock::wait`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    /// The whole span passed.
    Elapsed,
    /// The watchdog was stopped first: the run ended in time.
    Stopped,
}

/// The watchdog's clock.
pub trait Clock: Send + 'static {
    /// Waits `span`, or less when `stop` gets a message or its sender is dropped.
    fn wait(&mut self, span: Duration, stop: &Receiver<()>) -> Waited;
}

/// The real clock: the wait is `stop`'s own timeout.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealClock;

impl Clock for RealClock {
    fn wait(&mut self, span: Duration, stop: &Receiver<()>) -> Waited {
        match stop.recv_timeout(span) {
            Err(RecvTimeoutError::Timeout) => Waited::Elapsed,
            Ok(()) | Err(RecvTimeoutError::Disconnected) => Waited::Stopped,
        }
    }
}

/// The running watchdog. Dropping it stops the thread and waits for it, so a run that ended in
/// time is never ended afterwards.
#[derive(Debug)]
pub struct Watchdog {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            // A watchdog that panicked has nothing left to do.
            let _joined = thread.join();
        }
    }
}

/// When the watchdog acts, for a run of `deadline`: the cancel at `deadline - R` (zero, at
/// once, when the deadline is `R` or less), then the end, `deadline` after the start.
pub fn schedule(deadline: Duration) -> (Duration, Duration) {
    let cancel_after = deadline.saturating_sub(ROLLBACK_BOUND);
    (cancel_after, deadline.saturating_sub(cancel_after))
}

/// Starts the watchdog for a run of `deadline` from now: `on_cancel` at `deadline - R` (here,
/// before returning, when that is now), then `terminate` at `deadline`. Either runs only if
/// the [`Watchdog`] is still alive. A thread that can't be started is the error, and then
/// `on_cancel` has run only if the deadline was `R` or less.
pub fn start(
    deadline: Duration,
    mut clock: impl Clock,
    on_cancel: impl FnOnce() + Send + 'static,
    terminate: impl FnOnce() + Send + 'static,
) -> std::io::Result<Watchdog> {
    let (cancel_after, then_end) = schedule(deadline);
    let mut on_cancel = Some(on_cancel);
    if cancel_after.is_zero()
        && let Some(cancel) = on_cancel.take()
    {
        tracing::warn!("the set-up's deadline is near; it makes no further change");
        cancel();
    }
    let (stop, stopped) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("setup-deadline".to_owned())
        .spawn(move || {
            if let Some(cancel) = on_cancel {
                if clock.wait(cancel_after, &stopped) == Waited::Stopped {
                    return;
                }
                tracing::warn!("the set-up's deadline is near; it makes no further change");
                cancel();
            }
            if clock.wait(then_end, &stopped) == Waited::Stopped {
                return;
            }
            tracing::error!("the set-up reached its deadline; ending it");
            terminate();
        })?;
    Ok(Watchdog {
        stop: Some(stop),
        thread: Some(thread),
    })
}

#[cfg(test)]
mod tests;
