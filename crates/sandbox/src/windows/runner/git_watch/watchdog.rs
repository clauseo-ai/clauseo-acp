//! The git watch's watchdog: the 2 s budget (`set_aside::SCAN_BUDGET`) held without the check's
//! help. A check keeps time only between items, so one call that blocks (an open an agent's
//! oplock holds up, the listing of a folder with millions of names) would stall the watch with
//! no limit while the agent runs, and nothing would say so. So the watch marks each check busy
//! from its start ([`Pulse::busy`]) and idle at its end, and a thread of its own wakes every
//! [`TICK`]: when one check has been busy past the budget and the session hasn't ended, it ends
//! the session ([`Dog::stall`]), as a panic would. The agent's processes then end, their handles
//! close with them, and the blocked call returns.
//!
//! [`Pulse`] is shared between the two threads, plain atomics; [`watchdog`] is the thread's
//! loop, over a [`Dog`] the tests fake (`outer/watch.rs` has the real one).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use crate::windows::set_aside::SCAN_BUDGET;

/// How often the watchdog looks.
pub(crate) const TICK: Duration = Duration::from_millis(250);

/// What the watch shares with its watchdog: since when the check under way has been busy, and
/// whether the session ended.
#[derive(Debug, Default)]
pub(crate) struct Pulse {
    /// The monotonic reading at the start of the check under way, in nanoseconds plus one;
    /// zero while no check runs.
    since: AtomicU64,
    ended: AtomicBool,
}

impl Pulse {
    /// A check starts at the monotonic reading `now`.
    pub(crate) fn busy(&self, now: Duration) {
        let nanos = u64::try_from(now.as_nanos()).unwrap_or(u64::MAX - 1);
        self.since.store(nanos.saturating_add(1), Ordering::SeqCst);
    }

    /// The check under way is done.
    pub(crate) fn idle(&self) {
        self.since.store(0, Ordering::SeqCst);
    }

    /// Marks the session ended; whether this was the first to: that one tells the lawyer why.
    pub(crate) fn end(&self) -> bool {
        !self.ended.swap(true, Ordering::SeqCst)
    }

    /// Whether, at the monotonic reading `now`, a check has been busy longer than `budget`
    /// while the session runs.
    pub(crate) fn stalled(&self, now: Duration, budget: Duration) -> bool {
        let since = self.since.load(Ordering::SeqCst);
        if since == 0 || self.ended.load(Ordering::SeqCst) {
            return false;
        }
        let since = Duration::from_nanos(since - 1);
        now.saturating_sub(since) > budget
    }
}

/// What the watchdog asks of the session.
pub(crate) trait Dog {
    /// Waits up to `wait`; `true`, early, once the watch is stopped.
    fn wait(&self, wait: Duration) -> bool;
    /// A monotonic reading, the one the watch marks [`Pulse::busy`] with.
    fn now(&self) -> Duration;
    /// Ends the session: a check stalled. Called once, after [`Pulse::end`]; `first` says
    /// whether the watchdog was first to end it, and so tells the lawyer why.
    fn stall(&self, first: bool);
}

/// The watchdog's loop, until the watch is stopped or a check stalls past [`SCAN_BUDGET`].
pub(crate) fn watchdog(pulse: &Pulse, dog: &impl Dog) {
    loop {
        if dog.wait(TICK) {
            return;
        }
        if pulse.stalled(dog.now(), SCAN_BUDGET) {
            tracing::warn!("a git watch check stayed busy past its budget; ending the session");
            dog.stall(pulse.end());
            return;
        }
    }
}
