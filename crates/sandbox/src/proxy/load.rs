//! The proxy's socket counts: client connections still reading their head (per listener, so the
//! token-free browser listener cannot lock out the agent's), and client and origin sockets per
//! launch and across the process, each held by a slot that releases it on drop.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::proxy::{Limits, Listener};

/// Client and origin sockets open across every proxy in the process, against
/// [`Limits::process`]: one launch cannot take the host's last file descriptors.
static PROCESS_SOCKETS: AtomicUsize = AtomicUsize::new(0);

/// One proxy's open sockets: client connections still reading their head, per listener, and
/// client and origin sockets together.
#[derive(Default)]
pub(crate) struct Load {
    /// Indexed by [`slot_of`]: the agent listener's, then the browser listener's.
    pending: [Cell<usize>; 2],
    sockets: Cell<usize>,
}

/// A listener's index in [`Load::pending`].
fn slot_of(which: Listener) -> usize {
    match which {
        Listener::Agent => 0,
        Listener::Browser => 1,
    }
}

impl Load {
    /// One socket against the proxy's and the process's caps, or `None` when either is full.
    pub(crate) fn socket(self: &Rc<Load>, limits: &Limits) -> Option<SocketSlot> {
        if self.sockets.get() >= limits.sockets {
            return None;
        }
        PROCESS_SOCKETS
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < limits.process).then_some(n + 1)
            })
            .ok()?;
        self.sockets.set(self.sockets.get() + 1);
        Some(SocketSlot(Rc::clone(self)))
    }

    /// One more connection reading its head on `which`, or `None` when that listener's cap
    /// is reached.
    pub(crate) fn pending(
        self: &Rc<Load>,
        limits: &Limits,
        which: Listener,
    ) -> Option<PendingSlot> {
        let index = slot_of(which);
        let count = self.pending.get(index)?;
        if count.get() >= limits.pending {
            return None;
        }
        count.set(count.get() + 1);
        Some(PendingSlot(Rc::clone(self), index))
    }
}

/// One socket counted against the proxy's and the process's caps, until it drops.
pub(crate) struct SocketSlot(Rc<Load>);

impl Drop for SocketSlot {
    fn drop(&mut self) {
        self.0.sockets.set(self.0.sockets.get().saturating_sub(1));
        PROCESS_SOCKETS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One client connection counted as not having finished its head, until it drops.
pub(crate) struct PendingSlot(Rc<Load>, usize);

impl Drop for PendingSlot {
    fn drop(&mut self) {
        if let Some(count) = self.0.pending.get(self.1) {
            count.set(count.get().saturating_sub(1));
        }
    }
}

/// The process-wide socket cap for a soft open-file limit (`None` when unlimited or unknown):
/// 1024, or half the limit when that is smaller, so the proxies leave the host the other half.
pub(crate) fn process_cap(soft_open_files: Option<u64>) -> usize {
    const MOST: usize = 1024;
    soft_open_files.map_or(MOST, |soft| {
        usize::try_from(soft / 2).map_or(MOST, |half| half.min(MOST))
    })
}

/// The process's soft open-file limit, `None` when unlimited (or on Windows, which has none).
#[cfg(unix)]
pub(crate) fn soft_open_files() -> Option<u64> {
    rustix::process::getrlimit(rustix::process::Resource::Nofile).current
}

/// The process's soft open-file limit: Windows has none the proxy can read.
#[cfg(not(unix))]
pub(crate) fn soft_open_files() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::process_cap;

    #[test]
    fn the_process_cap_is_1024_or_half_the_soft_open_file_limit() {
        assert_eq!(process_cap(None), 1024);
        assert_eq!(
            process_cap(Some(256)),
            128,
            "launchd's default for a GUI app"
        );
        assert_eq!(process_cap(Some(2048)), 1024);
        assert_eq!(process_cap(Some(10_240)), 1024);
        assert_eq!(process_cap(Some(1500)), 750);
        assert_eq!(process_cap(Some(u64::MAX)), 1024);
    }
}
