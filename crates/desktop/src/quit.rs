//! The quit's pieces (`lib.rs`'s `ExitRequested` stops the app through `stop_host_then_exit`):
//! the agents' installs and sign-ins are shut down first (`setup`, `BackendCell::shutdown`),
//! then the host stops ([`stop_in_order`]).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::setup;

/// Where exit stands: a stopper started, and the host stopped (so the next request exits).
#[derive(Default)]
pub(crate) struct Exit {
    pub(crate) stopping: AtomicBool,
    pub(crate) stopped: AtomicBool,
}

/// The setup backend the quit shuts down: `Agents`' installs and sign-ins.
pub(crate) struct SetupStop(pub(crate) Arc<setup::BackendCell>);

/// How long the quit waits for the agents' jobs to end and their children to be killed.
pub(crate) const AGENTS_STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

/// The quit's two steps in their order: `agents` shuts `Agents` down (its answer says whether
/// its jobs ended in time), then `host` stops the host. A job still running is logged, and the
/// host stops all the same.
pub(crate) fn stop_in_order(agents: impl FnOnce() -> bool, host: impl FnOnce()) {
    if !agents() {
        tracing::warn!("an agents job was still running at quit");
    }
    host();
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::stop_in_order;
    use crate::bridge::lock;

    #[test]
    fn the_agents_shut_down_before_the_host_stops() {
        let steps = Mutex::new(Vec::new());
        stop_in_order(
            || {
                lock(&steps).push("agents");
                true
            },
            || lock(&steps).push("host"),
        );
        assert_eq!(*lock(&steps), ["agents", "host"]);
    }

    #[test]
    fn a_job_still_running_at_quit_does_not_keep_the_host_up() {
        let steps = Mutex::new(Vec::new());
        stop_in_order(
            || {
                lock(&steps).push("agents");
                false
            },
            || lock(&steps).push("host"),
        );
        assert_eq!(*lock(&steps), ["agents", "host"]);
    }
}
