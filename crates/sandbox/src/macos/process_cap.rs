//! The agent's process cap on macOS. Seatbelt cannot count processes (it has `process-fork`,
//! allow or deny, and nothing else), and `RLIMIT_NPROC` counts the user's whole process count,
//! so neither caps one agent. Three parts do it instead (0038, "The process cap"):
//!
//! - the launch: the agent leads a session of its own (`setsid`), with `RLIMIT_NPROC` soft and
//!   hard at [`process_limit`]: the backstop, which keeps [`RESERVE`] of the user's process slots
//!   for the lawyer's own apps whatever the agent does;
//! - the watchdog (`process_cap/watchdog.rs`), a thread of this process, which counts the
//!   processes of the agent's session every [`POLL`], and every [`SWEEP_EVERY`] polls also
//!   walks every process's parent for descendants that made sessions of their own;
//! - at [`AGENT_PROCESS_CAP`], it stops every descendant, kills them, keeps the agent unless
//!   [`verdict`] says the agent is the one forking or the session's trips reached
//!   [`ESCALATE_AT`], and tells the lawyer through the session's [`ProcessCapSession`].

use std::collections::{HashMap, HashSet};
use std::time::Duration;

pub(crate) mod libproc;
mod session;
pub(crate) mod watchdog;

pub use session::{ProcessCapNotice, ProcessCapSession};

/// The most descendants an agent may have running at once, Windows' job cap too (its
/// `ACTIVE_PROCESS_CAP`), so both platforms tell the lawyer at the same count. Legitimate agent
/// work (npm, pip, Python multiprocessing, `make -j`, jest workers) peaks well under 100; a fork
/// bomb passes 256 within milliseconds. macOS enforces it after the fact: a bomb is frozen
/// within one poll, measured at about 760 processes at 50 ms.
pub(crate) const AGENT_PROCESS_CAP: usize = 256;

/// The trip, counted per lawyer session, from which the agent is ended too: twice a command may
/// bomb and the agent go on to report it; a third time the agent is starting them again and
/// again, and each round costs the Mac a few hundred processes and a burst of listings.
pub(crate) const ESCALATE_AT: u32 = 3;

/// The user's process slots the agent can never take: what the lawyer keeps to open a window, a
/// Terminal tab or an app while a bomb runs.
pub(crate) const RESERVE: u64 = 512;

/// The system's process slots (`kern.maxproc`) left to other users' processes: root's daemons
/// and the `_`-role users count against `kern.maxproc` (fork fails for every non-root process at
/// `kern.maxproc` less one, xnu `fork1`), not against the lawyer's `kern.maxprocperuid`. An idle
/// Mac mini runs 183 of them (measured); 512 leaves them room to grow almost threefold while the
/// agent sits at its limit.
pub(crate) const SYSTEM_MARGIN: u64 = 512;

/// The least room the launch accepts ([`process_limit`]): [`RESERVE`] for the lawyer, and as
/// much again for the agent and the lawyer's running apps (about 430 on an idle Mac, measured).
pub(crate) const MIN_ROOM: u64 = 1_024;

/// How often the watchdog counts the agent's session: one `proc_listallpids` and a `getsid` per
/// process, about 66 µs for 607 processes (measured), 0.13 % of one core. At most one listing
/// runs per poll outside a trip, however much the agent forks.
pub(crate) const POLL: Duration = Duration::from_millis(50);

/// Every how many polls (2 s) the watchdog also walks every process's parent (`proc_pidinfo`
/// each, about 380 µs), for descendants that left the session.
pub(crate) const SWEEP_EVERY: u32 = 40;

/// The agent's `RLIMIT_NPROC`, soft and hard: the user's room (`kern.maxprocperuid`, `per_user`,
/// and `kern.maxproc`, `max_proc`, less [`SYSTEM_MARGIN`], whichever is less) less [`RESERVE`],
/// never above this process's own hard limit `hard` (a non-root child cannot raise it).
///
/// Refused when it leaves the agent less than [`MIN_ROOM`] less [`RESERVE`]: the room under
/// [`MIN_ROOM`], or a hard limit so low (`ulimit -Hu 300`) that the agent could not fork past
/// the lawyer's own count. The error names what the agent would have had, reserve included.
pub(crate) fn process_limit(
    per_user: u64,
    max_proc: u64,
    hard: u64,
) -> Result<u64, clauseo_process::Error> {
    let room = per_user.min(max_proc.saturating_sub(SYSTEM_MARGIN));
    let allowed = room.min(hard.saturating_add(RESERVE));
    if allowed < MIN_ROOM {
        return Err(clauseo_process::Error::ProcessLimitTooLow {
            allowed,
            needed: MIN_ROOM,
        });
    }
    Ok(allowed - RESERVE)
}

/// One running process, as `proc_pidinfo(PROC_PIDTBSDINFO)` and `getsid` read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Proc {
    /// Its pid.
    pub(crate) pid: u32,
    /// Its parent's pid.
    pub(crate) parent: u32,
    /// Its session.
    pub(crate) session: u32,
    /// Its start, in microseconds since the epoch: with the pid it names one process, as a
    /// reused pid has a later start.
    pub(crate) start: u64,
    /// Its real user id: the session sweep asks only about this user's processes.
    pub(crate) uid: u32,
}

/// The descendants of `root` (which started at `root_start`) among `all`, the root left out:
/// every member of the root's session that started no earlier than the root, and every process
/// reached by parent pid from the root or from one of those, each started no earlier than its
/// parent (an earlier one is a reused pid, not a child).
///
/// The session finds what job control (`set -m`, `setpgid`) moved to a group of its own, orphans
/// launchd adopted included: only `setsid` leaves a session, and nothing outside the agent's
/// tree can join it (it leads its own). Its id stays the agent's while one member lives, as xnu
/// gives no new process a pid that names a live session (`kern_fork.c`, `forkproc`). The parent
/// walk finds a `setsid` descendant while its parent lives; an orphan that called `setsid` is
/// found by neither (the session sweep's, a later change).
pub(crate) fn descendants(root: u32, root_start: u64, all: &[Proc]) -> Vec<Proc> {
    let mut children: HashMap<u32, Vec<&Proc>> = HashMap::new();
    for process in all.iter().filter(|p| p.pid != root) {
        children.entry(process.parent).or_default().push(process);
    }
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    let mut next = vec![(root, root_start)];
    for process in all {
        let member = process.session == root && process.pid != root && process.start >= root_start;
        if member && seen.insert(process.pid) {
            found.push(*process);
            next.push((process.pid, process.start));
        }
    }
    while let Some((parent, start)) = next.pop() {
        for child in children.get(&parent).into_iter().flatten() {
            if child.start >= start && seen.insert(child.pid) {
                found.push(**child);
                next.push((child.pid, child.start));
            }
        }
    }
    found
}

/// What a trip does to the agent itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The agent stays: its turn goes on and reports the failed command, as on Windows.
    SpareAgent,
    /// The agent is ended too: it is the one forking, or its session tripped too often.
    KillAgent,
}

/// The rule for trip number `trip` of the session (from 1): the agent is ended from trip
/// [`ESCALATE_AT`] on, and before that when its own children are more than half of the
/// descendants `found`. A bomb a command starts is the command's children and grandchildren,
/// and its agent has a handful of children; an agent that starts processes in a loop is most of
/// its tree's parent.
pub(crate) fn verdict(trip: u32, root: u32, found: &[Proc]) -> Verdict {
    let own = found.iter().filter(|p| p.parent == root).count();
    if trip >= ESCALATE_AT || own * 2 > found.len() {
        Verdict::KillAgent
    } else {
        Verdict::SpareAgent
    }
}

/// Where the watchdog leaves the descendants its bounded kill rounds did not end (a process
/// still forking, or one it could not read): the session sweep takes them over
/// (`sweep::take_over`), with every process the launch's mark still finds, the agent left out.
/// What still runs after it is logged, and the watchdog goes on: its next poll counts them
/// again, and its next sweep finds them by session, parent and mark.
pub(crate) fn hand_over_to_session_sweep(root: u32, left: &[Proc]) {
    let running = crate::macos::sweep::take_over(root, left);
    if running > 0 {
        tracing::warn!(
            agent = root,
            left = running,
            "the agent's process cap could not end every descendant; it keeps sweeping"
        );
    }
}

#[cfg(test)]
mod tests;
