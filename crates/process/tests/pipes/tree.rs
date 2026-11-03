//! The tree dies with its agent, and with its handle, against `fake_agent`'s `grandchild`
//! scenario: the agent starts `fake_agent --hold` with null stdio and says its pid on stderr.

use async_io::block_on;
use clauseo_process::Launched;
use futures::join;

use crate::exit_watch::{self, Watch};
use crate::harness::{RunDir, launch, lines};

/// The exit code `TerminateJobObject` gives (crates/process/src/os/windows.rs `TERMINATED`):
/// a grandchild with it was ended by the handle's tree kill, not by the job closing.
#[cfg(windows)]
const JOB_KILLED: u32 = 0xC1A0;

/// Waits for the grandchild, inside the test's script so a pipe the grandchild may hold (it
/// inherits the agent's handles on Windows) cannot keep the test waiting first. On Windows the
/// wait has a deadline and the grandchild must carry the tree kill's exit code.
fn wait_for_the_tree_kill(grandchild: Watch) {
    #[cfg(windows)]
    assert_eq!(
        grandchild.wait(),
        JOB_KILLED,
        "the grandchild was not ended by the tree kill"
    );
    #[cfg(not(windows))]
    grandchild.wait();
}

#[test]
fn grandchild_dies_with_its_agent() {
    exit_watch::before_launch();
    let dir = RunDir::new();
    let Launched {
        stdout: _stdout,
        stderr,
        mut handle,
        ..
    } = launch("grandchild", &dir).unwrap();
    let (pumped, mut lines) = lines(stderr);
    let script = async {
        let grandchild = lines.grandchild().await;
        // Armed while the grandchild runs, so a failure below still kills it.
        let watch = Watch::new(grandchild);
        // The agent alone dies, as in a crash; `exited` must take the rest of the tree.
        exit_watch::kill_only(handle.pid());
        let report = handle.exited().await;
        wait_for_the_tree_kill(watch);
        report
    };
    let (pumped, report) = block_on(async { join!(pumped, script) });
    pumped.unwrap();
    #[cfg(unix)]
    assert_eq!(
        report.signal(),
        Some(nix::sys::signal::Signal::SIGKILL as i32)
    );
    #[cfg(windows)]
    assert_eq!(report.code(), Some(1));
}

#[test]
fn drop_kills_the_tree() {
    exit_watch::before_launch();
    let dir = RunDir::new();
    let Launched {
        stdin,
        stdout,
        stderr,
        handle,
        ..
    } = launch("grandchild", &dir).unwrap();
    let (pumped, mut lines) = lines(stderr);
    let script = async {
        let grandchild = Watch::new(lines.grandchild().await);
        drop((stdin, stdout, handle));
        // On Windows the code says the handle's `Drop` killed the tree; closing the job, which
        // the handle's fields do next, would kill it too, with another code.
        wait_for_the_tree_kill(grandchild);
    };
    // Stderr ends once the agent is gone; the grandchild holds none of its pipes on Unix, and
    // on Windows it dies with the job.
    let (pumped, ()) = block_on(async { join!(pumped, script) });
    pumped.unwrap();
}
