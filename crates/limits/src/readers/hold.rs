//! The hold on one agent's reads (decision record 0048): while a sign-in, a sign-out or a key
//! change rewrites the login, no read of that agent may run, so none reads half a login.

#[cfg(test)]
mod tests;

use std::future::{Future, poll_fn};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Poll, Waker};

/// One agent's gate: whether its reads are held, and how many run.
#[derive(Default)]
pub(crate) struct Gate {
    inner: Mutex<Inner>,
}

/// What a [`Gate`] keeps under its lock.
#[derive(Default)]
struct Inner {
    /// Reads that start now wait for [`Gate::resume`].
    held: bool,
    /// Reads past the gate and not yet ended.
    running: usize,
    /// Every task waiting on a change: a held read, or a hold waiting for reads to end.
    waiting: Vec<Waker>,
}

impl Gate {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Holds the reads from now on (at this call, not at the first poll), and answers a future
    /// that resolves once no read is running. Reads that start meanwhile wait for
    /// [`Gate::resume`]. Holding twice is holding once.
    pub(crate) fn hold(self: &Arc<Self>) -> impl Future<Output = ()> + Send + 'static {
        self.lock().held = true;
        let gate = Arc::clone(self);
        poll_fn(move |cx| {
            let mut inner = gate.lock();
            if inner.running == 0 {
                Poll::Ready(())
            } else {
                inner.waiting.push(cx.waker().clone());
                Poll::Pending
            }
        })
    }

    /// Lets held reads go, and every read after them. Resuming a gate not held does nothing.
    pub(crate) fn resume(&self) {
        let waiting = {
            let mut inner = self.lock();
            inner.held = false;
            std::mem::take(&mut inner.waiting)
        };
        waiting.into_iter().for_each(Waker::wake);
    }

    /// Waits while the reads are held, then counts one read running until the answer drops.
    pub(crate) fn enter(self: Arc<Self>) -> impl Future<Output = Running> + Send + 'static {
        poll_fn(move |cx| {
            let mut inner = self.lock();
            if inner.held {
                inner.waiting.push(cx.waker().clone());
                Poll::Pending
            } else {
                inner.running += 1;
                drop(inner);
                Poll::Ready(Running(Arc::clone(&self)))
            }
        })
    }
}

/// One read past the gate; dropping it ends the read's count, and the last one wakes a hold.
pub(crate) struct Running(Arc<Gate>);

impl Drop for Running {
    fn drop(&mut self) {
        let waiting = {
            let mut inner = self.0.lock();
            inner.running = inner.running.saturating_sub(1);
            if inner.running == 0 {
                std::mem::take(&mut inner.waiting)
            } else {
                Vec::new()
            }
        };
        waiting.into_iter().for_each(Waker::wake);
    }
}
