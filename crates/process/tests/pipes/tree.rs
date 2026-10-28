//! The tree dies with its agent, and with its handle, against `fake_agent`'s `grandchild`
//! scenario: the agent starts `fake_agent --hold` with null stdio and says its pid on stderr.

use async_io::block_on;
use clauseo_process::Launched;
use futures::join;

use crate::exit_watch::{self, Watch};
use crate::harness::{RunDir, launch, lines};

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
        (handle.exited().await, watch)
    };
    let (pumped, (report, grandchild)) = block_on(async { join!(pumped, script) });
    pumped.unwrap();
    #[cfg(unix)]
    assert_eq!(
        report.signal(),
        Some(nix::sys::signal::Signal::SIGKILL as i32)
    );
    #[cfg(windows)]
    assert_eq!(report.code(), Some(1));
    grandchild.wait();
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
        grandchild
    };
    // Stderr ends once the agent is gone; the grandchild holds none of its pipes on Unix, and
    // on Windows it dies with the job.
    let (pumped, grandchild) = block_on(async { join!(pumped, script) });
    pumped.unwrap();
    grandchild.wait();
}
