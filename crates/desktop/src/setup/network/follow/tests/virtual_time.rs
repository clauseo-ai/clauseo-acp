//! Virtual time for the lookups' budget and the probes, shared by the tests that would otherwise
//! race a real deadline on a loaded machine.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use super::super::super::LookupTime;
use super::super::super::ask::Event;
use crate::bridge::lock;

/// The longest a test waits in real time: a guard against a hang, never a budget check.
pub(super) const HANG_GUARD: Duration = Duration::from_secs(10);

/// Virtual time for the lookups' budget and the probes: it moves only when a probe takes its
/// time, or when every lookup left hangs (`hang`) and no probe is running, to the end of the
/// wait. Else the lookups answer at once and are waited for.
#[derive(Default)]
pub(super) struct Virtual {
    now: Mutex<Duration>,
    hang: AtomicBool,
}

impl Virtual {
    pub(super) fn advance(&self, by: Duration) {
        *lock(&self.now) += by;
    }

    /// Whether the lookups still running hang: then the wait for them ends at its deadline.
    pub(super) fn hang(&self, hang: bool) {
        self.hang.store(hang, Ordering::SeqCst);
    }
}

impl LookupTime for Virtual {
    fn now(&self) -> Duration {
        *lock(&self.now)
    }

    fn next(
        &self,
        events: &mpsc::Receiver<Event>,
        until: Duration,
        probing: usize,
    ) -> Option<Event> {
        if let Ok(event) = events.try_recv() {
            return Some(event);
        }
        if probing > 0 || !self.hang.load(Ordering::SeqCst) {
            let event = events.recv_timeout(HANG_GUARD);
            return Some(event.expect("an answer or a probe result within the hang guard"));
        }
        let mut now = lock(&self.now);
        *now = (*now).max(until);
        None
    }
}
