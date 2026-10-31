//! What the exit report's stderr tail shows of an agent's stderr, with the agent writing exactly
//! the bytes the test chose: it is this test binary run again (`support/self_agent.rs`), reading
//! its stderr from its stdin.

#[cfg(test)]
#[path = "support/self_agent.rs"]
mod self_agent;

use std::io::{Read as _, Write as _};
use std::path::PathBuf;

use async_io::block_on;
use clauseo_process::{Launched, SpawnOptions};
use futures::{AsyncWriteExt, join};

/// The agent the test here starts, not a check: run as one, it reads its stdin to the end,
/// writes what it read to stderr as it is and returns. In a normal run it returns at once.
#[test]
fn agent_copies_its_stdin_to_stderr() {
    if !self_agent::is_agent() {
        return;
    }
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .expect("read stdin");
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(&input).expect("write stderr");
    stderr.flush().expect("flush stderr");
}

/// The home folder given in `SpawnOptions::home` shows as `~` in the stderr tail.
#[test]
fn home_folder_shows_as_tilde_in_the_tail() {
    let home = "/home/clauseo-test-user";
    let opts = SpawnOptions::new().home(PathBuf::from(home));
    let Launched {
        mut stdin,
        stdout: _stdout,
        stderr,
        mut handle,
        ..
    } = self_agent::launch("agent_copies_its_stdin_to_stderr", &opts).unwrap();
    let line = format!("cannot read {home}/project/notes.md\n");
    let (report, pumped) = block_on(async {
        stdin.write_all(line.as_bytes()).await.unwrap();
        stdin.close().await.unwrap();
        join!(handle.exited(), stderr.drain())
    });
    pumped.unwrap();
    assert_eq!(report.code(), Some(0));
    assert_eq!(report.stderr_tail(), "cannot read ~/project/notes.md\n");
}
