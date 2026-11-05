//! The hold on one agent's reads (decision record 0048): while a sign-in, a sign-out or a key
//! change rewrites the login, no read of that agent may run, so none reads half a login.
//!
//! A hold is a [`HoldGuard`]: the reads stay held until every guard taken is dropped, so a hold
//! cannot be left on by a forgotten call, and two overlapping holds need two drops.

#[cfg(test)]
mod tests;

use std::future::{Future, poll_fn};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Poll, Waker};

/// One agent's gate: how many holds it has, and how many reads run.
#[derive(Default)]
pub(crate) struct Gate {
    inner: Mutex<Inner>,
}

/// What a [`Gate`] keeps under its lock.
#[derive(Default)]
struct Inner {
    /// Holds not yet dropped. Reads that start while any is left wait.
    holds: usize,
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
    /// that resolves to the hold's guard once no read is running. Reads that start meanwhile
    /// wait until every guard is dropped. Dropping the future before it resolves drops the
    /// hold with it.
    pub(crate) fn hold(self: &Arc<Self>) -> impl Future<Output = HoldGuard> + Send + 'static {
        self.lock().holds += 1;
        let guard = HoldGuard(Arc::clone(self));
        async move {
            poll_fn(|cx| {
                let mut inner = guard.0.lock();
                if inner.running == 0 {
                    Poll::Ready(())
                } else {
                    inner.waiting.push(cx.waker().clone());
                    Poll::Pending
                }
            })
            .await;
            guard
        }
    }

    /// Waits while the reads are held, then counts one read running until the answer drops.
    pub(crate) fn enter(self: Arc<Self>) -> impl Future<Output = Running> + Send + 'static {
        poll_fn(move |cx| {
            let mut inner = self.lock();
            if inner.holds > 0 {
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

/// One hold on an agent's reads, from [`crate::ProcessReaders::hold`]. Dropping it ends the
/// hold; once the agent's last hold drops, its held reads go, and every read after them. The
/// host keeps it across the whole login change, so a change that fails or returns early still
/// lets the reads go.
#[must_use = "dropping the guard ends the hold at once"]
pub struct HoldGuard(Arc<Gate>);

impl Drop for HoldGuard {
    fn drop(&mut self) {
        let waiting = {
            let mut inner = self.0.lock();
            inner.holds = inner.holds.saturating_sub(1);
            if inner.holds == 0 {
                std::mem::take(&mut inner.waiting)
            } else {
                Vec::new()
            }
        };
        waiting.into_iter().for_each(Waker::wake);
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
