//! The host start's threads at below-normal priority on Windows, so on a machine with few cores
//! the host's start takes only the CPU `WebView2`'s processes and the main thread leave: the
//! host now starts while `WebView2` builds the main window (`lib.rs`'s setup), and on a 2-vCPU
//! machine the two competed.
//!
//! - Each thread that does start-up work lowers itself as it begins ([`lower_current`]): the
//!   host-start thread (`host_start::start_on_thread`), the Documents lookup's
//!   (`host_start::documents_later`) and the setup's (`setup::spawn`). A Windows thread starts at
//!   normal priority whatever its parent's, so each thread must lower itself.
//! - The host-start thread holds [`restore_on_drop`]'s guard; when its start ends, ok, failed or
//!   panicking, the guard puts every lowered thread back to normal and closes the lowering, so a
//!   thread that would lower itself later stays normal and the host never stays slow.
//! - Not covered: the host's own threads (`clauseo-host`'s actor and the store's writer and
//!   reader, which `Host::start` spawns in those crates) start at normal priority.
//! - One `clauseo_desktop::startup::lifecycle` line per lowered thread (`start-up: start thread
//!   lowered`, with its name) and one at the restore (`start-up: start threads restored`, with
//!   how many), each with `at_ms`. macOS and Linux lower nothing and log nothing.

use std::sync::Mutex;

use crate::chooser::trace::at_ms;
use crate::startup::TARGET;

#[cfg(windows)]
mod windows;

#[cfg(test)]
mod tests;

/// How a platform lowers the calling thread and restores a lowered one.
pub(crate) trait Priority {
    /// What restoring a thread needs (on Windows, a real handle to it).
    type Thread: Send;
    /// Lowers the calling thread; `None` when it was not lowered.
    fn lower_current(&self) -> Option<Self::Thread>;
    /// Puts `thread` back to normal priority and releases it.
    fn restore(&self, thread: Self::Thread);
}

/// The threads lowered for the start, until the restore closes it (`None` from then on).
pub(crate) struct Lowering<P: Priority> {
    os: P,
    lowered: Mutex<Option<Vec<P::Thread>>>,
}

impl<P: Priority> Lowering<P> {
    pub(crate) const fn new(os: P) -> Self {
        Self {
            os,
            lowered: Mutex::new(Some(Vec::new())),
        }
    }

    /// Lowers the calling thread, named `name` in the log, unless the restore has run. True when
    /// it was lowered.
    pub(crate) fn lower_current(&self, name: &str) -> bool {
        let mut lowered = self
            .lowered
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(threads) = lowered.as_mut() else {
            return false;
        };
        let Some(thread) = self.os.lower_current() else {
            return false;
        };
        threads.push(thread);
        tracing::info!(target: TARGET, at_ms = at_ms(), thread = name, "start-up: start thread lowered");
        true
    }

    /// The guard that restores this lowering when dropped.
    pub(crate) const fn restore_on_drop(&self) -> Restore<'_, P> {
        Restore(self)
    }

    /// Restores every lowered thread and closes the lowering; how many it restored. A second
    /// call restores none.
    pub(crate) fn restore_all(&self) -> usize {
        let threads = self
            .lowered
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .unwrap_or_default();
        let restored = threads.len();
        for thread in threads {
            self.os.restore(thread);
        }
        if restored > 0 {
            tracing::info!(target: TARGET, at_ms = at_ms(), threads = restored, "start-up: start threads restored");
        }
        restored
    }
}

/// Restores [`Lowering::restore_all`] when dropped: at the end of the host's start, whatever its
/// outcome, a panic's unwinding included.
pub(crate) struct Restore<'a, P: Priority>(&'a Lowering<P>);

impl<P: Priority> Drop for Restore<'_, P> {
    fn drop(&mut self) {
        self.0.restore_all();
    }
}

/// The OS's priorities: below normal and back on Windows, nothing elsewhere.
pub(crate) struct Os;

#[cfg(not(windows))]
impl Priority for Os {
    type Thread = ();
    fn lower_current(&self) -> Option<()> {
        None
    }
    fn restore(&self, (): ()) {}
}

/// The app's one lowering.
static START: Lowering<Os> = Lowering::new(Os);

/// Lowers the calling thread for the host's start (see the module), named `name` in the log.
pub(crate) fn lower_current(name: &str) {
    START.lower_current(name);
}

/// The guard the host-start thread holds: dropped, it restores every lowered thread.
pub(crate) fn restore_on_drop() -> Restore<'static, Os> {
    START.restore_on_drop()
}
