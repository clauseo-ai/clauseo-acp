//! `ManualClock`: a clock tests move by hand, with timers that wake futures on any thread.

use std::collections::{BTreeMap, VecDeque};
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use clauseo_protocol::Timestamp;

use super::{Clock, Now};

/// A clock that moves only when a test calls [`ManualClock::advance`]. Clones share one time.
/// Its timers wake their futures from whichever thread advances it, so a host running on its
/// own thread sees them fire.
#[derive(Clone, Debug)]
pub struct ManualClock {
    inner: Arc<Mutex<State>>,
}

/// A timer's place in the queue: its deadline, then its arming order.
type Key = (Duration, u64);

#[derive(Debug)]
struct State {
    start_millis: i64,
    mono: Duration,
    next_id: u64,
    /// Timers not yet fired, by deadline then arming order, with the waker of their last poll.
    pending: BTreeMap<Key, Option<Waker>>,
    /// Delays armed and not yet returned by `next_armed`, in arming order.
    armed_log: VecDeque<Duration>,
    /// Tasks waiting in `next_armed` for a timer to be armed.
    armed_waiters: Vec<Waker>,
}

fn lock(inner: &Mutex<State>) -> MutexGuard<'_, State> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ManualClock {
    /// A clock whose wall time starts at `start` and whose monotonic time starts at zero.
    pub fn new(start: Timestamp) -> Self {
        Self {
            inner: Arc::new(Mutex::new(State {
                start_millis: start.as_millis(),
                mono: Duration::ZERO,
                next_id: 0,
                pending: BTreeMap::new(),
                armed_log: VecDeque::new(),
                armed_waiters: Vec::new(),
            })),
        }
    }

    /// Moves wall and monotonic time forward by `d`, then fires every timer now due, in
    /// deadline order (ties in arming order).
    pub fn advance(&self, d: Duration) {
        let due = {
            let mut state = lock(&self.inner);
            state.mono = state.mono.saturating_add(d);
            let mono = state.mono;
            let later = state.pending.split_off(&(mono, u64::MAX));
            std::mem::replace(&mut state.pending, later)
        };
        for waker in due.into_values().flatten() {
            waker.wake();
        }
    }

    /// The delay of the next timer armed that no earlier call returned; waits until one is.
    pub async fn next_armed(&self) -> Duration {
        poll_fn(|cx| {
            let mut state = lock(&self.inner);
            if let Some(delay) = state.armed_log.pop_front() {
                return Poll::Ready(delay);
            }
            if !state.armed_waiters.iter().any(|w| w.will_wake(cx.waker())) {
                state.armed_waiters.push(cx.waker().clone());
            }
            Poll::Pending
        })
        .await
    }

    /// How many timers are armed and not yet fired or dropped.
    pub fn armed(&self) -> usize {
        lock(&self.inner).pending.len()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Now {
        let state = lock(&self.inner);
        let millis = i64::try_from(state.mono.as_millis()).unwrap_or(i64::MAX);
        let wall =
            Timestamp::new(state.start_millis.saturating_add(millis)).unwrap_or(Timestamp::ZERO);
        Now {
            wall,
            mono: state.mono,
        }
    }

    fn after(&self, d: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let (key, waiters) = {
            let mut state = lock(&self.inner);
            let key = (state.mono.saturating_add(d), state.next_id);
            state.next_id = state.next_id.saturating_add(1);
            state.pending.insert(key, None);
            state.armed_log.push_back(d);
            (key, std::mem::take(&mut state.armed_waiters))
        };
        for waker in waiters {
            waker.wake();
        }
        Box::pin(ManualTimer {
            inner: Arc::clone(&self.inner),
            key,
        })
    }
}

/// One armed timer; dropping it disarms it.
struct ManualTimer {
    inner: Arc<Mutex<State>>,
    key: Key,
}

impl Future for ManualTimer {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = lock(&self.inner);
        let (deadline, _) = self.key;
        if deadline <= state.mono {
            state.pending.remove(&self.key);
            return Poll::Ready(());
        }
        match state.pending.get_mut(&self.key) {
            Some(slot) => {
                *slot = Some(cx.waker().clone());
                Poll::Pending
            }
            None => Poll::Ready(()),
        }
    }
}

impl Drop for ManualTimer {
    fn drop(&mut self) {
        lock(&self.inner).pending.remove(&self.key);
    }
}
