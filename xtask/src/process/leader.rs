//! The leader: xtask's own binary in the hidden `process-leader` mode. It stops each command's
//! process group when xtask dies, however it dies (SIGKILL, a panic, Ctrl-C), as
//! `ui/app/scripts/buildtime-leader.mjs` does for the scripts.
//!
//! One leader serves the whole xtask process. It starts with the first command, in a process group
//! of its own (so a signal to xtask's group does not reach it), with stdin the read end of a pipe
//! xtask keeps open for its whole life and stdout and stderr closed (so it holds none of the pipes
//! xtask's parent reads). Before a command's start returns, xtask writes `+<group>`; when its run
//! has ended, `-<group>`. When the pipe closes, which happens only when xtask has exited, the
//! leader sends SIGTERM to every group still registered, waits up to `KILL_GRACE` for them to
//! empty, then sends SIGKILL to each and exits. One leader, not one per command, because
//! `cargo xtask check` starts dozens of commands: each then costs one short write instead of a
//! second process.
//!
//! The leader also inherits every descriptor xtask inherited without close-on-exec, and holds it
//! until it exits.
//!
//! Not covered: a command whose start returned in xtask but whose `+<group>` write had not happened
//! when xtask died (a few instructions after `spawn`), a process that left its group (`setsid`),
//! and a leader that is itself killed. Ctrl-Z stops xtask but not its commands.

use super::running::KILL_GRACE;
use super::spawn;
use super::unix::signal;
use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// The hidden subcommand that runs the leader. `main.rs` names its variant `ProcessLeader`, which
/// clap spells this way; `the_leader_subcommand_parses` ties them.
pub(super) const LEADER_SUBCOMMAND: &str = "process-leader";

/// How often a stopping leader checks whether the groups it signalled are empty.
const POLL: Duration = Duration::from_millis(50);

/// The leader of this xtask process, once a command has started.
struct Leader {
    /// The write end of the leader's stdin. It closes when xtask exits, and only then.
    pipe: io::PipeWriter,
    /// Kept, never waited for: the leader outlives xtask.
    child: Child,
}

static LEADER: Mutex<Option<Leader>> = Mutex::new(None);

/// Starts the leader if it is not running yet. Called before each command starts, so its
/// `register` is one write.
pub(super) fn ensure() -> io::Result<()> {
    let mut leader = LEADER.lock().unwrap_or_else(PoisonError::into_inner);
    if leader.is_none() {
        *leader = Some(start()?);
    }
    Ok(())
}

/// Tells the leader to stop `group` if xtask dies.
pub(super) fn register(group: Pid) -> io::Result<()> {
    let mut leader = LEADER.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(leader) = leader.as_mut() else {
        return Err(io::Error::other(
            "xtask's process leader was not started before the command",
        ));
    };
    leader
        .pipe
        .write_all(format!("+{group}\n").as_bytes())
        .map_err(|error| {
            // Not the write error's kind: callers read `NotFound` and `TimedOut` as their own.
            io::Error::other(format!(
                "could not tell xtask's process leader (pid {}) about process group {group}, so \
                 the command was stopped: {error}",
                leader.child.id()
            ))
        })
}

/// Tells the leader that `group` is no longer xtask's to stop. A leader that cannot be told has
/// died, and has no group left to stop.
pub(super) fn release(group: Pid) {
    let mut leader = LEADER.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(leader) = leader.as_mut() {
        drop(leader.pipe.write_all(format!("-{group}\n").as_bytes()));
    }
}

/// Starts the leader.
fn start() -> io::Result<Leader> {
    let exe = leader_exe()?;
    let (reader, pipe) = io::pipe()?;
    let mut command = Command::new(&exe);
    command
        .arg(LEADER_SUBCOMMAND)
        .stdin(reader)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    // Not the spawn error's kind: a caller would read `NotFound` as its own program missing.
    let child = spawn(&mut command).map_err(|error| {
        io::Error::other(format!(
            "could not start xtask's process leader {}: {error}",
            exe.display()
        ))
    })?;
    Ok(Leader { pipe, child })
}

/// The xtask binary, which runs the leader.
#[cfg(not(test))]
fn leader_exe() -> io::Result<PathBuf> {
    std::env::current_exe()
}

/// The xtask binary, which runs the leader. The unit tests run from
/// `target/<profile>/deps/xtask-<hash>`, which is not xtask, so the leader is the xtask binary
/// cargo builds in `target/<profile>/` for the tests in `xtask/tests/`. Found from the test
/// binary's own path, so a copied tree (cargo-mutants) finds its own.
#[cfg(test)]
fn leader_exe() -> io::Result<PathBuf> {
    let harness = std::env::current_exe()?;
    let name = format!("xtask{}", std::env::consts::EXE_SUFFIX);
    let exe = harness
        .parent()
        .and_then(std::path::Path::parent)
        .map(|profile| profile.join(name));
    match exe {
        Some(exe) if exe.is_file() => Ok(exe),
        _ => Err(io::Error::other(format!(
            "the unit tests run xtask's process leader from the xtask binary next to the deps/ \
             folder of {}, and it is missing: run them with `cargo test -p xtask` or \
             `cargo nextest run -p xtask`, which build it for xtask/tests/",
            harness.display()
        ))),
    }
}

/// The hidden `process-leader` mode: reads `+<group>` and `-<group>` lines from stdin until it
/// closes, then stops the groups still registered.
pub(crate) fn lead() -> ExitCode {
    let groups = registered(io::stdin().lock());
    stop(&groups);
    ExitCode::SUCCESS
}

/// The groups `input` registers and does not release, read until it ends. A line that is neither
/// is skipped: only xtask writes it.
fn registered(input: impl BufRead) -> BTreeSet<i32> {
    let mut groups = BTreeSet::new();
    for line in input.lines() {
        let Ok(line) = line else { break };
        let (sign, group) = line.split_at_checked(1).unwrap_or(("", ""));
        match (sign, group.parse::<i32>()) {
            ("+", Ok(group)) if group > 0 => {
                groups.insert(group);
            }
            ("-", Ok(group)) => {
                groups.remove(&group);
            }
            _ => {}
        }
    }
    groups
}

/// SIGTERM to each of `groups`, then, after up to `KILL_GRACE` for them to empty, SIGKILL to each.
fn stop(groups: &BTreeSet<i32>) {
    let groups: Vec<Pid> = groups.iter().map(|group| Pid::from_raw(*group)).collect();
    for group in &groups {
        drop(signal(*group, Signal::SIGTERM));
    }
    // Never sent to while `_keep` lives, so each receive times out after `POLL`.
    let (_keep, tick) = mpsc::channel::<()>();
    let mut waited = Duration::ZERO;
    while waited < KILL_GRACE {
        if groups.iter().all(|group| empty(*group)) {
            return;
        }
        if tick.recv_timeout(POLL) != Err(RecvTimeoutError::Timeout) {
            break;
        }
        waited += POLL;
    }
    for group in &groups {
        drop(signal(*group, Signal::SIGKILL));
    }
}

/// Whether no process is left in `group`. `EPERM` means one is, owned by another user.
fn empty(group: Pid) -> bool {
    killpg(group, None) == Err(Errno::ESRCH)
}

#[cfg(test)]
mod tests;
