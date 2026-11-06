//! The setup host thread's followers, run again when one panics (R17 review L2). A panic in
//! any follower unwinds them all (they run joined on one thread), and nothing would then keep
//! the identity check's refusals in step: so the identity check fails closed first
//! (`identity::fail_closed`: `agent.identity_unknown` on every agent it watches), with an error
//! log in fixed words, and the followers start again, at most [`RESTARTS`] times. Past that the
//! refusals stay until Clauseo starts again.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::backend::AgentsBackend;
use super::identity;

/// How many times the followers start again after a panic.
pub(crate) const RESTARTS: usize = 3;

/// Runs `run` (the followers, until they end), again after each panic, as the module docs say.
pub(crate) fn supervise(backend: &AgentsBackend, mut run: impl FnMut()) {
    for restart in 0..=RESTARTS {
        if catch_unwind(AssertUnwindSafe(&mut run)).is_ok() {
            return;
        }
        tracing::error!(
            restart,
            "the setup followers panicked: identity checks fail closed"
        );
        let closed = catch_unwind(AssertUnwindSafe(|| {
            futures::executor::block_on(identity::fail_closed(backend));
        }));
        if closed.is_err() {
            tracing::error!("the identity check could not fail closed");
        }
    }
    tracing::error!("the setup followers stopped: launches stay refused until Clauseo restarts");
}
