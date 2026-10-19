//! `ignore-stdin-close` and `grandchild`, for S7's shutdown and tree-kill tests.

use crate::harness::{Agent, Scratch, kill_pid};

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
    let written = std::fs::read_to_string(dir.path().join("grandchild.pid")).unwrap();
    assert_eq!(written, format!("{pid}\n"));
    assert_ne!(pid, agent.pid());
    agent.kill();
    kill_pid(pid);
    let finished = agent.finish();
    assert!(!finished.status.success());
}
