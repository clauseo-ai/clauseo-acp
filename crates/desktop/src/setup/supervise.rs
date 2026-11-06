//! The setup host thread's followers, run again when one panics (R17 review L2, L4). A panic
//! in any follower unwinds them all (they run joined on one thread), and nothing would then
//! keep the identity check's refusals in step: so the identity check fails closed first
//! (`identity::fail_closed`: `agent.identity_unknown` on every agent it watches), with an error
//! log in fixed words, and the followers start again, at most [`RESTARTS`] times. Past that
//! the identity check is dead (`Identities::die`): every agent on the shared login stays
//! refused `agent.identity_unknown` until Clauseo starts again, whatever the pump or the launch
//! gate does meanwhile, and one error line says so.
//!
//! **Debug and test builds only.** The workspace's `[profile.release]` sets `panic = "abort"`,
//! so in a release build a follower's panic aborts the app, and none of this runs: no restart,
//! no dead flag. The code stays because the tests drive it with injected panics.

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
        if restart == RESTARTS {
            // Pinned before the refusals are told, so nothing lifts them after.
            backend.wiring.identity.die();
        }
        close(backend);
    }
    tracing::error!(
        "the setup followers stopped: launches on a shared login stay refused until Clauseo restarts"
    );
}

/// `identity::fail_closed`, a panic in it logged.
fn close(backend: &AgentsBackend) {
    let closed = catch_unwind(AssertUnwindSafe(|| {
        futures::executor::block_on(identity::fail_closed(backend));
    }));
    if closed.is_err() {
        tracing::error!("the identity check could not fail closed");
    }
}
