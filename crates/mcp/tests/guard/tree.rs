//! Windows: a daemon-port mismatch ends agent-browser's whole tree, its detached grandchild
//! included, through the guard's Job Object (`src/browser_guard/job.rs`).

use crate::support::{DAEMON_PORT, Scratch, example, guard_command, run};
use crate::write;

/// `tree_stub` starts a detached grandchild (as agent-browser starts its daemon) that holds the
/// guard's stderr open, then relays one line. The daemon's `.port` file already names another
/// port, so the guard refuses after that line and ends the job. `run` reads stderr to its end,
/// which comes only once the grandchild is gone; a grandchild that outlived the guard would hang
/// the test until nextest's timeout ends it.
#[test]
fn a_mismatch_ends_the_detached_grandchild_too() {
    let scratch = Scratch::new("tree");
    let run_dir = scratch.cwd().join("run");
    write::create_dir_all(&run_dir).unwrap();
    write::write(run_dir.join(format!("{}.port", scratch.session())), "50123").unwrap();
    assert_ne!(DAEMON_PORT, "50123");
    let args: [&std::ffi::OsStr; 0] = [];
    let output = run(guard_command(&scratch, &example("tree_stub"), &args));
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("agent-browser's daemon port file says 50123, not the claimed port 61102"),
        "{stderr}"
    );
    let pid = stderr
        .lines()
        .find_map(|line| line.strip_prefix("tree_stub grandchild "))
        .expect("the stub names its grandchild");
    // Its stderr closed, so it has ended; `tasklist` agrees.
    let mut tasklist = std::process::Command::new("tasklist");
    tasklist.args(["/FI", &format!("PID eq {pid}"), "/NH"]);
    let listed = String::from_utf8_lossy(&run(tasklist).stdout).into_owned();
    assert!(!listed.contains("tree_stub"), "{listed}");
}
