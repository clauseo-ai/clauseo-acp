//! `ignore-stdin-close` and `grandchild`, for S7's shutdown and tree-kill tests.

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

use crate::harness::{Agent, PidGuard, Scratch};

#[test]
fn ignore_stdin_close_keeps_running() {
    let dir = Scratch::new("ignore_stdin_close_keeps_running");
    let mut agent = Agent::start("ignore-stdin-close", dir.path());
    agent.handshake(&dir.cwd());
    agent.close_stdin();
    // An agent that exits on EOF never writes this line, so the read hits EOF and fails.
    assert_eq!(agent.stderr_line(), "fake_agent: stdin closed, holding");
    assert!(agent.running());
    agent.kill();
    let finished = agent.finish();
    assert!(!finished.status.success());
}

#[test]
fn grandchild_writes_pid() {
    let dir = Scratch::new("grandchild_writes_pid");
    let mut agent = Agent::start("grandchild", dir.path());
    let line = agent.stderr_line();
    let pid: u32 = line
        .strip_prefix("fake_agent: child ")
        .unwrap()
        .parse()
        .unwrap();
    // Armed before any assert, so a failure below still kills the grandchild.
    let grandchild = PidGuard::new(pid);
    let written = std::fs::read_to_string(dir.path().join("grandchild.pid")).unwrap();
    assert_eq!(written, format!("{pid}\n"));
    assert_ne!(pid, agent.pid());
    agent.kill();
    grandchild.kill();
    let finished = agent.finish();
    assert!(!finished.status.success());
}

/// `--hold` says so on stderr, then never exits. Killed, it dies by the kill (signal 9 on
/// Unix, exit 1 from `TerminateProcess` on Windows). A broken `--hold` prints usage and exits 2.
#[test]
fn hold_announces_itself_and_dies_when_killed() {
    let dir = Scratch::new("hold_announces_itself");
    let mut agent = Agent::start_args(&["--hold"], dir.path());
    assert_eq!(agent.stderr_line(), "fake_agent: holding");
    agent.kill();
    let finished = agent.finish();
    #[cfg(unix)]
    assert_eq!(finished.status.signal(), Some(9), "{:?}", finished.status);
    #[cfg(windows)]
    assert_eq!(finished.status.code(), Some(1), "{:?}", finished.status);
}
