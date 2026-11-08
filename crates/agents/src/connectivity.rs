//! "Connectivity may have improved" hints for an install waiting offline ([`ConnectivityWatch`]).
//! The job subscribes while it is `Waiting` and, on a hint, tries again after a short debounce
//! instead of at the next 30 s retry (the timer stays the backstop).
//!
//! On Windows the watch is `NotifyNetworkConnectivityHintChange` (`windows.rs`); elsewhere it is
//! [`NoWatch`], and the 30 s timer alone retries.

#[cfg(windows)]
mod windows;

use std::sync::Arc;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

/// Sends "connectivity may have improved" hints into a waiting install job.
pub trait ConnectivityWatch: Send + Sync {
    /// Starts sending on `hint` at each change toward connected, until the returned
    /// [`Subscription`] drops. `None` when the platform has no such notice (or it would not
    /// start): the job then retries on its timer alone.
    fn subscribe(&self, hint: Hint) -> Option<Subscription>;
}

/// The sending end a [`ConnectivityWatch`] is given. Sending never blocks and takes no lock
/// held across other work, so it may be called from an OS callback thread.
#[derive(Debug, Clone)]
pub struct Hint(UnboundedSender<()>);

impl Hint {
    /// Sends one hint; `false` once the job has stopped listening.
    pub fn send(&self) -> bool {
        self.0.unbounded_send(()).is_ok()
    }
}

/// A live subscription: dropping it unsubscribes, and once the drop returns no more hints are
/// sent.
pub struct Subscription {
    _guard: Box<dyn Send>,
}

impl Subscription {
    /// A subscription that `guard`'s drop ends.
    pub fn new(guard: impl Send + 'static) -> Subscription {
        Subscription {
            _guard: Box::new(guard),
        }
    }
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Subscription")
    }
}

/// The watch of a platform with none: never subscribes.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoWatch;

impl ConnectivityWatch for NoWatch {
    fn subscribe(&self, _hint: Hint) -> Option<Subscription> {
        None
    }
}

/// This platform's watch: the OS's connectivity hints on Windows, [`NoWatch`] elsewhere.
pub(crate) fn platform_watch() -> Arc<dyn ConnectivityWatch> {
    #[cfg(windows)]
    {
        Arc::new(windows::HintWatch)
    }
    #[cfg(not(windows))]
    {
        Arc::new(NoWatch)
    }
}

/// A waiting job's end of a subscription: the hints, and the subscription itself, which ends
/// when this drops.
#[derive(Debug)]
pub(crate) struct Listening {
    pub(crate) hints: UnboundedReceiver<()>,
    _subscription: Option<Subscription>,
}

impl Listening {
    /// Subscribes to `watch`.
    pub(crate) fn start(watch: &dyn ConnectivityWatch) -> Listening {
        let (sender, hints) = unbounded();
        let subscription = watch.subscribe(Hint(sender));
        Listening {
            hints,
            _subscription: subscription,
        }
    }

    /// Drops every hint already queued; `true` when there was one.
    pub(crate) fn drain(&mut self) -> bool {
        let mut any = false;
        while self.hints.try_recv().is_ok() {
            any = true;
        }
        any
    }
}
