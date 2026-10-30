//! The waits of the shutdown ladder, behind a trait so tests decide when they end.

use std::future::Future;
use std::time::Duration;

/// Gives the futures the shutdown ladder waits on.
pub trait Timer {
    /// A future that ends once `duration` has passed.
    fn after(&self, duration: Duration) -> impl Future<Output = ()> + Send;
}

/// The real timer: [`async_io::Timer::after`]. It needs no particular executor: async-io drives
/// its timers from its own thread.
#[derive(Debug, Clone, Copy, Default)]
pub struct AsyncIoTimer;

impl Timer for AsyncIoTimer {
    fn after(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        let timer = async_io::Timer::after(duration);
        async move {
            timer.await;
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub use manual::ManualTimer;

#[cfg(any(test, feature = "test-support"))]
mod manual {
    use std::collections::VecDeque;
    use std::future::Future;
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::task::{Poll, Waker};
    use std::time::Duration;

    use futures::channel::oneshot;

    use super::Timer;

    /// A timer that ends a wait only when the test says so. Each [`Timer::after`] arms a wait;
    /// [`ManualTimer::next_armed`] reads the next armed wait's duration, and
    /// [`ManualTimer::fire`] ends the wait it read last. Tests reach a step by reading a pipe or a
    /// stderr line, never by sleeping.
    #[derive(Debug, Default)]
    pub struct ManualTimer {
        state: Mutex<State>,
    }

    #[derive(Debug, Default)]
    struct State {
        /// Armed waits not read yet, oldest first.
        unread: VecDeque<(Duration, oneshot::Sender<()>)>,
        /// The task waiting in `next_armed`.
        reader: Option<Waker>,
        /// The wait `next_armed` read last.
        current: Option<oneshot::Sender<()>>,
        armed: usize,
        fired: usize,
    }

    impl ManualTimer {
        /// A timer with nothing armed.
        pub fn new() -> ManualTimer {
            ManualTimer::default()
        }

        fn lock(&self) -> MutexGuard<'_, State> {
            self.state.lock().unwrap_or_else(PoisonError::into_inner)
        }

        /// Waits for the next wait to be armed, oldest first, and gives its duration.
        pub fn next_armed(&self) -> impl Future<Output = Duration> + '_ {
            futures::future::poll_fn(|cx| {
                let mut state = self.lock();
                if let Some((duration, fire)) = state.unread.pop_front() {
                    state.current = Some(fire);
                    Poll::Ready(duration)
                } else {
                    state.reader = Some(cx.waker().clone());
                    Poll::Pending
                }
            })
        }

        /// Ends the wait [`ManualTimer::next_armed`] read last. Returns whether a wait was still
        /// there to end: false when none was read, or its future was dropped.
        pub fn fire(&self) -> bool {
            let mut state = self.lock();
            let fired = state
                .current
                .take()
                .is_some_and(|fire| fire.send(()).is_ok());
            state.fired += usize::from(fired);
            fired
        }

        /// How many waits [`Timer::after`] has armed.
        pub fn armed(&self) -> usize {
            self.lock().armed
        }

        /// How many waits [`ManualTimer::fire`] has ended.
        pub fn fired(&self) -> usize {
            self.lock().fired
        }
    }

    impl Timer for ManualTimer {
        fn after(&self, duration: Duration) -> impl Future<Output = ()> + Send {
            let (fire, fired) = oneshot::channel();
            let reader = {
                let mut state = self.lock();
                state.unread.push_back((duration, fire));
                state.armed += 1;
                state.reader.take()
            };
            if let Some(reader) = reader {
                reader.wake();
            }
            async move {
                // A timer dropped before it fired never ends the wait.
                if fired.await.is_err() {
                    futures::future::pending::<()>().await;
                }
            }
        }
    }
}
