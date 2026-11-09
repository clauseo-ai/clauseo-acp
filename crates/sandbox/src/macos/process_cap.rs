//! The agent's process cap on macOS. Seatbelt cannot count processes (it has `process-fork`,
//! allow or deny, and nothing else), and `RLIMIT_NPROC` counts the user's whole process count,
//! so neither caps one agent. Three parts do it instead (0038, "The process cap"):
//!
//! - the launch: the agent leads a session of its own (`setsid`), with `RLIMIT_NPROC` soft and
//!   hard at [`process_limit`]: the backstop, which keeps [`RESERVE`] of the user's process slots
//!   for the lawyer's own apps whatever the agent does;
//! - the watchdog (`process_cap/watchdog.rs`), a thread of this process, which counts the
//!   agent's descendants every [`POLL`] and on each fork of the agent, and by a parent walk over
//!   every process every [`SWEEP`];
//! - at [`AGENT_PROCESS_CAP`], it stops every descendant, kills them, keeps the agent unless
//!   [`verdict`] says the agent is the one forking, and tells the lawyer once a session.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

pub(crate) mod libproc;
pub(crate) mod watchdog;

/// The most descendants an agent may have running at once, Windows' job cap too (its
/// `ACTIVE_PROCESS_CAP`), so both platforms tell the lawyer at the same count. Legitimate agent
/// work (npm, pip, Python multiprocessing, `make -j`, jest workers) peaks well under 100; a fork
/// bomb passes 256 within milliseconds. macOS enforces it after the fact: a bomb is frozen
/// within one poll, measured at about 760 processes at 50 ms.
pub(crate) const AGENT_PROCESS_CAP: usize = 256;

/// The user's process slots the agent can never take (`kern.maxprocperuid` less this is its
/// `RLIMIT_NPROC`): what the lawyer keeps to open a window, a Terminal tab or an app while a
/// bomb runs.
pub(crate) const RESERVE: u64 = 512;

/// The least `kern.maxprocperuid` the launch accepts: [`RESERVE`] for the lawyer, and as much
/// again for the agent and the lawyer's running apps (about 430 on an idle Mac, measured).
pub(crate) const MIN_PER_USER: u64 = 1_024;

/// How often the watchdog counts the agent's process group (about 17 µs a count, measured).
pub(crate) const POLL: Duration = Duration::from_millis(50);

/// How often the watchdog walks every process's parent, for descendants that left the group.
pub(crate) const SWEEP: Duration = Duration::from_secs(2);

/// What the lawyer sees, once per session, when the cap stopped a task: Windows' text
/// (`windows/runner/job_port.rs`), word for word.
pub(crate) const TOO_MANY_PROGRAMS: &str = "A task tried to start too many programs at once, so \
                                            Clauseo stopped it. Ask the assistant to try a \
                                            smaller step.";

/// Where the notice goes: the launcher's, set by `SeatbeltLauncher::on_too_many_programs`.
pub(crate) type Notify = Arc<dyn Fn(&str) + Send + Sync>;

/// The agent's `RLIMIT_NPROC`, soft and hard, from `kern.maxprocperuid` (`per_user`) and this
/// process's own hard limit (`hard`): `per_user` less [`RESERVE`], never above `hard` (a
/// non-root child cannot raise it). Refused when `per_user` is under [`MIN_PER_USER`].
pub(crate) fn process_limit(per_user: u64, hard: u64) -> Result<u64, clauseo_process::Error> {
    if per_user < MIN_PER_USER {
        return Err(clauseo_process::Error::ProcessLimitTooLow {
            per_user,
            needed: MIN_PER_USER,
        });
    }
    Ok((per_user - RESERVE).min(hard))
}

/// One running process, as `proc_pidinfo(PROC_PIDTBSDINFO)` reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Proc {
    /// Its pid.
    pub(crate) pid: u32,
    /// Its parent's pid.
    pub(crate) parent: u32,
    /// Its process group.
    pub(crate) group: u32,
    /// Its start, in microseconds since the epoch: with the pid it names one process, as a
    /// reused pid has a later start.
    pub(crate) start: u64,
}

/// The descendants of `root` (which started at `root_start`) among `all`, the root left out:
/// those reached by parent pid from it, each started no earlier than its parent (an earlier one
/// is a reused pid, not a child), and every member of the root's process group that started no
/// earlier than the root (only the agent's descendants can join its group: it leads its own
/// session). An orphan that left the group is reparented to launchd and is not found.
pub(crate) fn descendants(root: u32, root_start: u64, all: &[Proc]) -> Vec<Proc> {
    let mut children: HashMap<u32, Vec<&Proc>> = HashMap::new();
    for process in all.iter().filter(|p| p.pid != root) {
        children.entry(process.parent).or_default().push(process);
    }
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    let mut next = vec![(root, root_start)];
    while let Some((parent, start)) = next.pop() {
        for child in children.get(&parent).into_iter().flatten() {
            if child.start >= start && seen.insert(child.pid) {
                found.push(**child);
                next.push((child.pid, child.start));
            }
        }
    }
    for process in all {
        let member = process.group == root && process.pid != root && process.start >= root_start;
        if member && seen.insert(process.pid) {
            found.push(*process);
        }
    }
    found
}

/// What a trip does to the agent itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The agent stays: its turn goes on and reports the failed command, as on Windows.
    SpareAgent,
    /// The agent is ended too: it is the one forking.
    KillAgent,
}

/// The rule: the agent is the one forking when its own children are more than half of the
/// descendants `found` at the trip. A bomb a command starts is the command's children and
/// grandchildren, and its agent has a handful of children; an agent that starts processes in a
/// loop is most of its tree's parent.
pub(crate) fn verdict(root: u32, found: &[Proc]) -> Verdict {
    let own = found.iter().filter(|p| p.parent == root).count();
    if own * 2 > found.len() {
        Verdict::KillAgent
    } else {
        Verdict::SpareAgent
    }
}

#[cfg(test)]
mod tests;
