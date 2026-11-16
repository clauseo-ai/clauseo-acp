//! A thread queue's state for the tests' dump (`sign_in/state_dump.rs`).

use super::ThreadQueue;

/// `queue`'s turns, holds and steers as counts and flags.
pub(in crate::actor::turn) fn describe_for_test(queue: &ThreadQueue) -> String {
    format!(
        "turns={} held={} sign_in={} requeue={} steers={} removing={}",
        queue.turns.len(),
        queue.held,
        queue.sign_in.is_some(),
        queue.requeue.is_some(),
        queue.steers.len(),
        queue.removing.len(),
    )
}
