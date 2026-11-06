//! Windows: a daemon-port mismatch ends agent-browser's whole tree, its detached grandchild
//! included, through the guard's Job Object (`src/browser_guard/job.rs`).

use serde_json::json;

use crate::support::{DAEMON_PORT, Guard, Scratch, example, guard_command, run, run_files};
use crate::write;

/// The browser starts lazily, at the first tool call (`src/browser_guard/session.rs`), and on
/// Windows the daemon's session name is the run's own, `run-<id>-<n>` (`runs::windows_session`),
/// so the mismatch goes in that name's `.port` file. `initialize` (a one-shot `tree_stub`, no
/// grandchild) shows the run is claimed; the test then names the run's session from its lock
/// file and writes the file with another port. The first `tools/call` starts Chrome's stand-in
/// and `tree_stub`, which starts a detached grandchild (as agent-browser starts its daemon) that
/// holds the guard's stderr open, then answers; the guard relays the answer, refuses and ends
/// the job. Reading stderr to its end returns only once the grandchild is gone; a grandchild that
/// outlived the guard would hang the test until nextest's timeout ends it.
#[test]
fn a_mismatch_ends_the_detached_grandchild_too() {
    let scratch = Scratch::new("tree");
    let args: [&std::ffi::OsStr; 0] = [];
    let mut guard = Guard::start(guard_command(&scratch, &example("tree_stub"), &args));
    guard.send(&json!({"jsonrpc": "2.0", "id": 0, "method": "initialize"}));
    assert_eq!(guard.read()["id"], 0);
    let run_dir = scratch.cwd().join("run");
    write::create_dir_all(&run_dir).unwrap();
    let session = run_session(&scratch.cwd());
    write::write(run_dir.join(format!("{session}.port")), "50123").unwrap();
    assert_ne!(DAEMON_PORT, "50123");
    guard.call("open");
    assert_eq!(guard.wait().code(), Some(1));
    let stderr = guard.stderr_text();
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

/// The run's agent-browser session name, as the guard picks it: the run's id from its lock file
/// `chrome-profile-<id>.lock`, then `run-<id>-<n>` with the smallest `n` whose derived daemon
/// port is the claimed one (`src/browser_guard/runs.rs`, `windows_session`).
fn run_session(cwd: &std::path::Path) -> String {
    let id = run_files(cwd)
        .iter()
        .find_map(|path| {
            let name = path.file_name()?.to_str()?;
            Some(
                name.strip_prefix("chrome-profile-")?
                    .strip_suffix(".lock")?
                    .to_owned(),
            )
        })
        .expect("the run's lock file");
    let port: u16 = DAEMON_PORT.parse().unwrap();
    (0..1_000_000)
        .map(|n| format!("run-{id}-{n}"))
        .find(|name| daemon_port_of(name) == port)
        .expect("a run name for the claimed daemon port")
}

/// The daemon port agent-browser derives from a session name on Windows (v0.41.1
/// `connection.rs:423-431`), as `runs::daemon_port_of` computes it: a Java string hash, folded
/// into 49152 to 65534.
fn daemon_port_of(session: &str) -> u16 {
    let mut hash: i32 = 0;
    for c in session.chars() {
        let c = i32::try_from(u32::from(c)).unwrap_or(i32::MAX);
        hash = (hash << 5).wrapping_sub(hash).wrapping_add(c);
    }
    let folded = u16::try_from(hash.unsigned_abs() % 16_383).unwrap_or(0);
    49_152 + folded
}
