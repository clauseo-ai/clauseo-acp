//! The proxy's socket counts: client connections still reading their head, and client and origin
//! sockets per launch and across the process, each held by a slot that releases it on drop.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::proxy::Limits;

/// Client and origin sockets open across every proxy in the process, against
/// [`Limits::process`]: one launch cannot take the host's last file descriptors.
static PROCESS_SOCKETS: AtomicUsize = AtomicUsize::new(0);

/// One proxy's open sockets: client connections still reading their head, and client and
/// origin sockets together.
#[derive(Default)]
pub(crate) struct Load {
    pending: Cell<usize>,
    sockets: Cell<usize>,
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

    /// One more connection reading its head, or `None` when the cap is reached.
    pub(crate) fn pending(self: &Rc<Load>, limits: &Limits) -> Option<PendingSlot> {
        if self.pending.get() >= limits.pending {
            return None;
        }
        self.pending.set(self.pending.get() + 1);
        Some(PendingSlot(Rc::clone(self)))
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
pub(crate) struct PendingSlot(Rc<Load>);

impl Drop for PendingSlot {
    fn drop(&mut self) {
        self.0.pending.set(self.0.pending.get().saturating_sub(1));
    }
}
