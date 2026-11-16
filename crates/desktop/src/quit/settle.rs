//! The quit's last wait for process starts, after the host stopped: `clauseo_process`'s gate
//! closes, then the quit waits at most [`STARTS_SETTLE_WAIT`] for its count of starts and held
//! handles to reach zero. A start counts before its child exists, so a zero means no child is
//! between its start and its tree. On Windows that moment matters: the child is created
//! suspended and joins its Job Object only after (`clauseo-process`'s `os/windows.rs`), so an
//! exit then leaves it suspended for good, in no job, its program file held (the next update
//! cannot replace it). stetdoc's background check (`profile/stetdoc.rs`), which no stop waits
//! for, is the one start a quit with nothing else running can catch there.

use std::time::Duration;

/// How long the quit waits for the count of starts to reach zero once the host stopped.
pub(crate) const STARTS_SETTLE_WAIT: Duration = Duration::from_secs(2);

/// How often the wait reads the count.
const POLL: Duration = Duration::from_millis(20);

/// Closes the gate (`clauseo_process::close_spawns`: no process starts here again), then waits
/// at most [`STARTS_SETTLE_WAIT`] for `clauseo_process::live_handles` to reach zero. Returns
/// whether it did; past the bound the quit goes on and logs the count.
pub(crate) fn starts() -> bool {
    clauseo_process::close_spawns();
    let settled = until_zero(
        clauseo_process::live_handles,
        STARTS_SETTLE_WAIT,
        POLL,
        sleep,
    );
    if !settled {
        tracing::warn!(
            live = clauseo_process::live_handles(),
            "processes were still held when the quit stopped waiting for them"
        );
    }
    settled
}

/// Whether `count` reads zero within `bound`: read now, then after each `sleep` of `poll` (the
/// last one cut to what is left of `bound`), so `count` is read once more at the bound itself.
pub(crate) fn until_zero(
    count: impl Fn() -> usize,
    bound: Duration,
    poll: Duration,
    mut sleep: impl FnMut(Duration),
) -> bool {
    let mut waited = Duration::ZERO;
    loop {
        if count() == 0 {
            return true;
        }
        let left = bound.saturating_sub(waited);
        if left.is_zero() || poll.is_zero() {
            return false;
        }
        let step = poll.min(left);
        sleep(step);
        waited += step;
    }
}

/// The real wait between two reads of the count.
#[expect(
    clippy::disallowed_methods,
    reason = "the quit's bounded wait for the last process start; the test injects its sleep"
)]
fn sleep(step: Duration) {
    std::thread::sleep(step);
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::time::Duration;

    use super::until_zero;

    const BOUND: Duration = Duration::from_secs(2);
    const POLL: Duration = Duration::from_millis(20);

    #[test]
    fn a_count_already_zero_returns_at_once_without_sleeping() {
        let slept = RefCell::new(Vec::new());
        assert!(until_zero(
            || 0,
            BOUND,
            POLL,
            |step| slept.borrow_mut().push(step)
        ));
        assert!(slept.borrow().is_empty());
    }

    #[test]
    fn the_wait_ends_when_the_count_reaches_zero() {
        // One start in progress, done after three polls.
        let left = Cell::new(3_usize);
        let slept = RefCell::new(Vec::new());
        let count = || left.get();
        let settled = until_zero(count, BOUND, POLL, |step| {
            slept.borrow_mut().push(step);
            left.set(left.get().saturating_sub(1));
        });
        assert!(settled);
        assert_eq!(*slept.borrow(), vec![POLL; 3]);
    }

    #[test]
    fn a_count_that_never_reaches_zero_ends_the_wait_at_the_bound() {
        let reads = Cell::new(0_usize);
        let slept = RefCell::new(Duration::ZERO);
        let settled = until_zero(
            || {
                reads.set(reads.get() + 1);
                1
            },
            Duration::from_millis(50),
            POLL,
            |step| *slept.borrow_mut() += step,
        );
        assert!(!settled);
        // 20 + 20 + the 10 left: read at 0, 20, 40 and at the bound.
        assert_eq!(*slept.borrow(), Duration::from_millis(50));
        assert_eq!(reads.get(), 4);
    }
}
