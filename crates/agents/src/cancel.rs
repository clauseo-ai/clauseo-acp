//! A stop signal for one job: checked between download chunks, and raced against every wait and
//! every child's run, so a stop ends the job where it is.
//!
//! It has two levels. [`Cancel::cancel`] stops at once: a run's child is killed as it sees it
//! (an install's pause, a sign-in a newer job replaced, the bound of a graceful stop).
//! [`Cancel::stop`] asks a sign-in's runs to end gracefully first ([`Stop`]): each run does what
//! its kind allows (`signin::run::Stopping`), and is killed only once [`Cancel::cancel`] follows,
//! or the stop's grace passes. Both set [`Cancel::is_cancelled`], so every job that checks it
//! winds down the same way.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Who asked a graceful stop, and how long a run may take to end before it is killed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The app's quit: `Agents::shutdown` kills what is left at its bound.
    Quit,
    /// The lawyer's Cancel: a run still going after the grace is killed by the run itself.
    Lawyer(Duration),
}

impl Stop {
    /// The grace the run keeps itself, if any (the quit's bound is kept by `shutdown`).
    pub(crate) fn grace(self) -> Option<Duration> {
        match self {
            Stop::Quit => None,
            Stop::Lawyer(grace) => Some(grace),
        }
    }

    /// A fixed word for the log.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Stop::Quit => "quit",
            Stop::Lawyer(_) => "cancel",
        }
    }
}

/// One level of the signal: set once, never cleared.
#[derive(Debug)]
struct Level {
    flag: AtomicBool,
    /// Dropped by [`Level::set`], which ends every [`Level::wait`].
    wake: Mutex<Option<async_channel::Sender<()>>>,
    woken: async_channel::Receiver<()>,
}

impl Level {
    fn new() -> Level {
        let (wake, woken) = async_channel::bounded(1);
        Level {
            flag: AtomicBool::new(false),
            wake: Mutex::new(Some(wake)),
            woken,
        }
    }

    fn set(&self) {
        self.flag.store(true, Ordering::SeqCst);
        let sender = self
            .wake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(sender);
    }

    fn is_set(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    async fn wait(&self) {
        // Nothing is ever sent: `recv` ends with an error once the sender is dropped.
        if self.woken.recv().await.is_err() {
            // The sender is gone: the level is set.
        }
    }
}

/// Set once, never cleared: a stopped job ends and a new job gets a new `Cancel`.
#[derive(Debug)]
pub(crate) struct Cancel {
    /// Any stop, graceful or not.
    stopped: Level,
    /// A stop at once: kill.
    killed: Level,
    /// The graceful stop's kind, when [`Cancel::stop`] set it first.
    how: Mutex<Option<Stop>>,
}

impl Cancel {
    /// A signal not yet set.
    pub(crate) fn new() -> Cancel {
        Cancel {
            stopped: Level::new(),
            killed: Level::new(),
            how: Mutex::new(None),
        }
    }

    /// Stops at once: sets both levels, and ends every [`Cancel::cancelled`] and
    /// [`Cancel::killed`] future.
    pub(crate) fn cancel(&self) {
        self.killed.set();
        self.stopped.set();
    }

    /// Asks a graceful stop of kind `how`: sets [`Cancel::is_cancelled`] but not
    /// [`Cancel::is_killed`]. A later stop does not change the first one's kind.
    pub(crate) fn stop(&self, how: Stop) {
        {
            let mut set = self
                .how
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if set.is_none() && !self.stopped.is_set() {
                *set = Some(how);
            }
        }
        self.stopped.set();
    }

    /// The graceful stop's kind, when one was asked before any kill.
    pub(crate) fn how(&self) -> Option<Stop> {
        *self
            .how
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Whether any stop is set.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.stopped.is_set()
    }

    /// Whether the stop at once is set.
    pub(crate) fn is_killed(&self) -> bool {
        self.killed.is_set()
    }

    /// Ends once any stop is set (at once if one already is).
    pub(crate) async fn cancelled(&self) {
        self.stopped.wait().await;
    }

    /// Ends once the stop at once is set (at once if it already is).
    pub(crate) async fn killed(&self) {
        self.killed.wait().await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Cancel, Stop};

    #[test]
    fn cancel_ends_the_wait() {
        let cancel = Cancel::new();
        assert!(!cancel.is_cancelled());
        cancel.cancel();
        assert!(cancel.is_cancelled());
        assert!(cancel.is_killed());
        assert_eq!(cancel.how(), None);
        futures::executor::block_on(cancel.cancelled());
        futures::executor::block_on(cancel.killed());
    }

    #[test]
    fn a_graceful_stop_is_cancelled_but_not_killed_until_the_kill() {
        let cancel = Cancel::new();
        cancel.stop(Stop::Lawyer(Duration::from_secs(2)));
        assert!(cancel.is_cancelled());
        assert!(!cancel.is_killed());
        futures::executor::block_on(cancel.cancelled());
        // A later stop keeps the first one's kind.
        cancel.stop(Stop::Quit);
        assert_eq!(cancel.how(), Some(Stop::Lawyer(Duration::from_secs(2))));
        cancel.cancel();
        assert!(cancel.is_killed());
    }

    #[test]
    fn a_stop_after_a_kill_has_no_grace() {
        let cancel = Cancel::new();
        cancel.cancel();
        cancel.stop(Stop::Quit);
        assert_eq!(cancel.how(), None);
    }
}
