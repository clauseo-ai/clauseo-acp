//! Whether a background try of stetdoc is in flight, for an agent launch to wait on
//! ([`super::recheck`]). The background thread holds an [`InFlight`] for each try and drops it
//! when the try ends: answered (after the caller put the new profile in place), timed out at its
//! limit, or not started. A launch asks [`Tries::wait`]: `None` when no try is in flight, or a
//! [`TryEnd`] that ends with the try. Nothing polls: the end is a channel the drop closes.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::Shared;

/// The end of one try: a receiver whose sender only the try's [`InFlight`] holds.
type End = Shared<oneshot::Receiver<()>>;

/// The try in flight, if any, with the number that tells it from the tries before it.
#[derive(Default)]
struct Slot {
    next: u64,
    current: Option<(u64, End)>,
}

/// The background tries of one app start; clones share them.
#[derive(Clone, Default)]
pub struct Tries(Arc<Mutex<Slot>>);

impl fmt::Debug for Tries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let in_flight = self.lock().current.is_some();
        f.debug_struct("Tries")
            .field("in_flight", &in_flight)
            .finish_non_exhaustive()
    }
}

impl Tries {
    /// Tries with none in flight.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks a try in flight until the [`InFlight`] it returns is dropped.
    pub fn begin(&self) -> InFlight {
        let (sender, receiver) = oneshot::channel();
        let mut slot = self.lock();
        let id = slot.next;
        slot.next += 1;
        slot.current = Some((id, receiver.shared()));
        InFlight {
            tries: self.clone(),
            id,
            _end: sender,
        }
    }

    /// The end of the try in flight now, or `None` when none is.
    #[must_use]
    pub fn wait(&self) -> Option<TryEnd> {
        let slot = self.lock();
        slot.current.as_ref().map(|(_, end)| TryEnd(end.clone()))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One try in flight: dropping it ends the try for every [`TryEnd`] that waits on it.
#[must_use = "the try ends when this is dropped"]
pub struct InFlight {
    tries: Tries,
    id: u64,
    _end: oneshot::Sender<()>,
}

impl fmt::Debug for InFlight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InFlight")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl InFlight {
    /// Ends this try and marks the next in flight with no gap between them, so a launch that
    /// asks in between waits on the next one.
    pub fn next(self) -> Self {
        let next = self.tries.begin();
        drop(self);
        next
    }
}

impl Drop for InFlight {
    /// Takes this try out of the slot when it is still the one there, then (as the fields drop)
    /// closes its channel, which ends every [`TryEnd`] on it.
    fn drop(&mut self) {
        let mut slot = self.tries.lock();
        if slot.current.as_ref().is_some_and(|(id, _)| *id == self.id) {
            slot.current = None;
        }
    }
}

/// A future that ends when the try it was taken from ends.
#[derive(Clone)]
pub struct TryEnd(End);

impl fmt::Debug for TryEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TryEnd")
    }
}

impl Future for TryEnd {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.0.poll_unpin(cx).map(|_closed| ())
    }
}

#[cfg(test)]
mod tests {
    use std::task::Waker;

    use super::*;

    fn ended(end: &mut TryEnd) -> bool {
        end.poll_unpin(&mut Context::from_waker(Waker::noop()))
            .is_ready()
    }

    #[test]
    fn no_try_in_flight_gives_no_wait() {
        let tries = Tries::new();
        assert!(tries.wait().is_none());
        drop(tries.begin());
        assert!(tries.wait().is_none(), "a try that ended leaves no wait");
    }

    #[test]
    fn a_wait_ends_when_its_try_ends() {
        let tries = Tries::new();
        let held = tries.begin();
        let mut end = tries.wait().unwrap();
        assert!(!ended(&mut end));
        drop(held);
        assert!(ended(&mut end));
    }

    #[test]
    fn next_ends_the_try_and_keeps_one_in_flight() {
        let tries = Tries::new();
        let held = tries.begin();
        let mut first = tries.wait().unwrap();
        let held = held.next();
        assert!(ended(&mut first), "the first try's wait ends");
        let mut second = tries.wait().expect("the next try is in flight at once");
        assert!(!ended(&mut second));
        drop(held);
        assert!(ended(&mut second));
        assert!(tries.wait().is_none());
    }
}
