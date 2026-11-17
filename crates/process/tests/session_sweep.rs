//! A session's leftover processes end with the session. The agent (`/bin/sh`, started with the
//! session's marker in its environment as the host starts agents) starts a writer that waits a
//! few seconds and then writes `late.txt` into the matter folder, and exits. The writer is
//! started in each of these shapes (`support/session_shapes.rs`):
//!
//! - `setsid`;
//! - a double fork, `(writer &) &`;
//! - `nohup writer &`;
//! - fork, `setsid`, fork: `sh -c '( setsid writer & ) &'`;
//! - a shell in a session of its own that runs `nohup writer &` and exits.
//!
//! Once the agent has exited (its group killed) and `SessionMark::sweep` has run, no
//! `late.txt` appears and nothing carries the marker. The writer reaches the agent in its
//! environment, so no shape nests one quoted command inside another.

#![cfg(any(target_os = "linux", target_os = "macos"))]

#[cfg(test)]
#[path = "support/session_shapes.rs"]
mod shapes;

/// The pipe tests' one module that changes files: here, each test's own scratch folder.
#[cfg(test)]
#[path = "pipes/write.rs"]
mod write;

use async_io::{Timer, block_on};
use clauseo_process::{AsyncIoTimer, SessionSwept};

use shapes::{WAIT, ends_with_the_session, mark, matter, remove, run_agent};

/// In a session of its own, its parent gone: the sweep ends it.
#[test]
fn a_setsid_leftover_ends_with_the_session() {
    let ended = ends_with_the_session(0, "Sharma v State 2024");
    assert!(ended >= 1, "the sweep ended nothing");
}

#[test]
fn a_double_forked_leftover_ends_with_the_session() {
    ends_with_the_session(1, "O'Brien & Sons");
}

#[test]
fn a_nohup_leftover_ends_with_the_session() {
    ends_with_the_session(2, "Sharma v State 2024");
}

/// In a session of its own, adopted by init: the sweep ends it.
#[test]
fn a_fork_setsid_fork_leftover_ends_with_the_session() {
    let ended = ends_with_the_session(3, "O'Brien & Sons");
    assert!(ended >= 1, "the sweep ended nothing");
}

/// In the group of a shell that had a session of its own and has exited: the sweep ends it.
#[test]
fn a_nohup_leftover_of_an_own_session_shell_ends_with_the_session() {
    let ended = ends_with_the_session(4, "Sharma v State 2024");
    assert!(ended >= 1, "the sweep ended nothing");
}

#[test]
fn the_sweep_spares_another_sessions_processes() {
    let folder = matter("spares", "O'Brien & Sons");
    let ours = mark(10);
    let theirs = mark(11);
    let (_, line) = shapes::shapes().into_iter().next().unwrap();
    // The other session's writer runs on while this session is swept.
    run_agent(&line, &folder, &theirs);
    assert_eq!(
        block_on(ours.sweep(&AsyncIoTimer)),
        SessionSwept::Swept {
            ended: 0,
            left: false
        }
    );
    block_on(Timer::after(WAIT));
    assert!(
        folder.join("late.txt").exists(),
        "another session's process was ended"
    );
    remove(folder.parent().unwrap());
}
