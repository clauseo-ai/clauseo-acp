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
//! A leader that died (someone killed it) is replaced at the next command's start, when the write
//! of its `+<group>` fails: xtask keeps the groups it registered, and tells them all to the new
//! leader.
//!
//! Not covered: a command whose start returned in xtask but whose `+<group>` write had not happened
//! when xtask died (a few instructions after `spawn`), a process that left its group (any
//! `setsid` or `setpgid`: cargo-mutants starts every cargo and nextest child in a group of its own,
//! its src/process/unix.rs:37 in 27.1.0, and cargo-nextest 0.9.148 each test, nextest-runner
//! 0.126.0's src/runner/executor.rs:433 and :828), and xtask dying while its leader is dead and
//! not yet replaced. Ctrl-Z stops xtask but not its commands.

use super::running::{KILL_GRACE, POLL};
use super::spawn;
use super::unix::{empty, signal};
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// The hidden subcommand that runs the leader. `main.rs` names its variant `ProcessLeader`, which
/// clap spells this way; `the_leader_subcommand_parses` ties them.
pub(super) const LEADER_SUBCOMMAND: &str = "process-leader";

/// The leader of this xtask process, once a command has started.
pub(super) struct Leader {
    /// The write end of the leader's stdin. It closes when xtask exits, or when a dead leader is
    /// replaced.
    pipe: io::PipeWriter,
    /// Waited for only once replaced: a live leader outlives xtask. `test_mode.rs` kills it to
    /// test its replacement.
    pub(super) child: Child,
    /// The groups registered and not released, which a new leader is told when this one dies.
    groups: BTreeSet<Pid>,
}

pub(super) static LEADER: Mutex<Option<Leader>> = Mutex::new(None);

impl Leader {
    /// A new leader, told every group of this one, which then gets SIGKILL and is reaped. Killed,
    /// not just let go: a live leader whose stdin closed would stop every group xtask still runs.
    /// On failure this one is kept, groups and all.
    fn replace(&mut self) -> io::Result<Leader> {
        let next = start(self.groups.clone())?;
        drop(self.child.kill());
        drop(self.child.wait());
        Ok(next)
    }
}

/// Starts the leader if it was never started. Called before each command starts, so its
/// `register` is one write.
pub(super) fn ensure() -> io::Result<()> {
    let mut slot = LEADER.lock().unwrap_or_else(PoisonError::into_inner);
    if slot.is_none() {
        *slot = Some(start(BTreeSet::new())?);
    }
    Ok(())
}

/// Tells the leader to stop `group` if xtask dies. A leader that cannot be told has died, and
/// is replaced.
pub(super) fn register(group: Pid) -> io::Result<()> {
    let mut slot = LEADER.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(leader) = slot.as_mut() else {
        return Err(io::Error::other(
            "xtask's process leader was not started before the command",
        ));
    };
    leader.groups.insert(group);
    let Err(error) = leader.pipe.write_all(format!("+{group}\n").as_bytes()) else {
        return Ok(());
    };
    match leader.replace() {
        Ok(next) => {
            *slot = Some(next);
            Ok(())
        }
        Err(start_error) => {
            leader.groups.remove(&group);
            // Not the errors' kind: callers read `NotFound` and `TimedOut` as their own.
            Err(io::Error::other(format!(
                "could not tell xtask's process leader (pid {}) about process group {group} \
                 ({error}), nor start a new one, so the command was stopped: {start_error}",
                leader.child.id()
            )))
        }
    }
}

/// Tells the leader that `group` is no longer xtask's to stop. A leader that cannot be told has
/// died, and stops nothing; a new one is not told the group.
pub(super) fn release(group: Pid) {
    let mut slot = LEADER.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(leader) = slot.as_mut() {
        leader.groups.remove(&group);
        drop(leader.pipe.write_all(format!("-{group}\n").as_bytes()));
    }
}

/// The leader's pid and the groups it was told about, for the tests.
#[cfg(test)]
fn current() -> Option<(u32, BTreeSet<Pid>)> {
    let slot = LEADER.lock().unwrap_or_else(PoisonError::into_inner);
    slot.as_ref()
        .map(|leader| (leader.child.id(), leader.groups.clone()))
}

/// Starts a leader and tells it `groups`.
fn start(groups: BTreeSet<Pid>) -> io::Result<Leader> {
    let exe = leader_exe()?;
    let (reader, mut pipe) = io::pipe()?;
    let mut command = Command::new(&exe);
    command
        .arg(LEADER_SUBCOMMAND)
        .stdin(reader)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    // Not the spawn error's kind: a caller would read `NotFound` as its own program missing.
    let mut child = spawn(&mut command).map_err(|error| {
        io::Error::other(format!(
            "could not start xtask's process leader {}: {error}",
            exe.display()
        ))
    })?;
    let mut told = String::new();
    for group in &groups {
        told.push('+');
        told.push_str(&group.to_string());
        told.push('\n');
    }
    if let Err(error) = pipe.write_all(told.as_bytes()) {
        drop(child.kill());
        drop(child.wait());
        return Err(io::Error::other(format!(
            "could not tell the new process leader of xtask the process groups it runs: {error}"
        )));
    }
    Ok(Leader {
        pipe,
        child,
        groups,
    })
}

/// `std::env::current_exe`'s answer, its error given a message and kind of its own: a caller
/// reads `NotFound` (Linux without `/proc`) as its own program missing.
fn own_binary(found: io::Result<PathBuf>) -> io::Result<PathBuf> {
    found.map_err(|error| {
        io::Error::other(format!(
            "could not find xtask's own binary, which runs its process leader: {error}"
        ))
    })
}

/// The xtask binary, which runs the leader.
#[cfg(not(test))]
fn leader_exe() -> io::Result<PathBuf> {
    own_binary(std::env::current_exe())
}

/// The xtask binary, which runs the leader. The unit tests run from
/// `target/<profile>/deps/xtask-<hash>`, which is not xtask, so the leader is the xtask binary
/// cargo builds in `target/<profile>/` for the tests in `xtask/tests/`. Found from the test
/// binary's own path, so a copied tree (cargo-mutants) finds its own.
#[cfg(test)]
fn leader_exe() -> io::Result<PathBuf> {
    let harness = own_binary(std::env::current_exe())?;
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
    stop(&groups, KILL_GRACE);
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

/// SIGTERM to each of `groups`, then, after up to `grace` (checked every `POLL`, as a run checks
/// its group) for them to empty, SIGKILL to each. The leader passes `KILL_GRACE`; tests pass a shorter one.
fn stop(groups: &BTreeSet<i32>, grace: Duration) {
    let groups: Vec<Pid> = groups.iter().map(|group| Pid::from_raw(*group)).collect();
    for group in &groups {
        drop(signal(*group, Signal::SIGTERM));
    }
    // Never sent to while `_keep` lives, so each receive times out after `POLL`.
    let (_keep, tick) = mpsc::channel::<()>();
    for _ in 0..grace.as_millis().div_ceil(POLL.as_millis()) {
        if groups.iter().all(|group| empty(*group)) {
            return;
        }
        // Never `Ok`: nothing sends.
        if tick.recv_timeout(POLL).is_ok() {
            break;
        }
    }
    for group in &groups {
        drop(signal(*group, Signal::SIGKILL));
    }
}

#[cfg(test)]
mod tests;
