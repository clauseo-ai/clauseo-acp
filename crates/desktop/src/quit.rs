//! The quit's pieces (`lib.rs`'s `ExitRequested` stops the app through `stop_host_then_exit`):
//! the agents' installs and sign-ins are shut down first (`setup`, `BackendCell::shutdown`),
//! then the host stops ([`stop_in_order`]).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use clauseo_host::HostExit;

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

/// The stopper thread's steps (`lib.rs`'s `stop_host_then_exit`): the quit's order
/// ([`stop_in_order`]: `agents`, then `host`, which answers how the host ended), then `flush`
/// (the host log, 0047), then `save_counts`, and last `after_host` with how the host ended, which
/// saves the window state and installs a downloaded update (`update::after_host`, 0037).
pub(crate) fn stop_then_after_host(
    agents: impl FnOnce() -> bool,
    host: impl FnOnce() -> HostExit,
    flush: impl FnOnce(),
    save_counts: impl FnOnce(),
    after_host: impl FnOnce(HostExit),
) {
    let mut ended = HostExit::Stopped;
    stop_in_order(agents, || ended = host());
    flush();
    save_counts();
    after_host(ended);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use clauseo_host::{Downloaded, HostExit, Opaque};

    use super::{stop_in_order, stop_then_after_host};
    use crate::bridge::lock;

    /// What `after_host` heard, as a step's name.
    fn heard(ended: &HostExit) -> String {
        match ended {
            HostExit::Install(downloaded) => format!("after_host install {}", downloaded.version),
            _ => "after_host stopped".to_owned(),
        }
    }

    /// The stopper's steps, each recording its name; `agents` answers `ended_in_time` and the
    /// host ends as `ended`.
    fn run_stopper(ended_in_time: bool, ended: HostExit) -> Vec<String> {
        let steps = Mutex::new(Vec::new());
        let step = |name: &str| lock(&steps).push(name.to_owned());
        stop_then_after_host(
            || {
                step("agents");
                ended_in_time
            },
            || {
                step("host");
                ended
            },
            || step("flush"),
            || step("counts"),
            |ended| step(&heard(&ended)),
        );
        steps.into_inner().unwrap()
    }

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

    #[test]
    fn the_install_runs_only_after_the_quit_s_order_the_log_flush_and_the_counts_save() {
        let downloaded = Downloaded {
            version: "0.4.1".to_owned(),
            path: PathBuf::from("updates/0.4.1/Clauseo-setup.exe"),
            handle: Opaque::new(()),
        };
        assert_eq!(
            run_stopper(true, HostExit::Install(downloaded)),
            [
                "agents",
                "host",
                "flush",
                "counts",
                "after_host install 0.4.1"
            ]
        );
    }

    #[test]
    fn a_plain_stop_with_a_job_still_running_reaches_after_host_as_stopped() {
        assert_eq!(
            run_stopper(false, HostExit::Stopped),
            ["agents", "host", "flush", "counts", "after_host stopped"]
        );
    }
}
