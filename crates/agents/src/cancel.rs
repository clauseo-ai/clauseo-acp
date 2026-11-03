//! A pause signal for one install job: checked between download chunks, and raced against every
//! wait and every npm run, so a pause stops the job where it is.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once, never cleared: a paused job ends and a new job gets a new `Cancel`.
#[derive(Debug)]
pub(crate) struct Cancel {
    flag: AtomicBool,
    /// Dropped by [`Cancel::cancel`], which ends every [`Cancel::cancelled`] future.
    wake: Mutex<Option<async_channel::Sender<()>>>,
    woken: async_channel::Receiver<()>,
}

impl Cancel {
    /// A signal not yet set.
    pub(crate) fn new() -> Cancel {
        let (wake, woken) = async_channel::bounded(1);
        Cancel {
            flag: AtomicBool::new(false),
            wake: Mutex::new(Some(wake)),
            woken,
        }
    }

    /// Sets it, and ends every [`Cancel::cancelled`] future.
    pub(crate) fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        let sender = self
            .wake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(sender);
    }

    /// Whether it is set.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Ends once it is set (at once if it already is).
    pub(crate) async fn cancelled(&self) {
        // Nothing is ever sent: `recv` ends with an error once the sender is dropped.
        if self.woken.recv().await.is_err() {
            // The sender is gone: the signal is set.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Cancel;

    #[test]
    fn cancel_ends_the_wait() {
        let cancel = Cancel::new();
        assert!(!cancel.is_cancelled());
        cancel.cancel();
        assert!(cancel.is_cancelled());
        futures::executor::block_on(cancel.cancelled());
    }
}
