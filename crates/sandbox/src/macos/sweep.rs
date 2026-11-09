//! The session sweep: finding and ending every process of a jailed launch, wherever it went: one
//! that called `setsid`, double-forked, closed every descriptor and was adopted by launchd is
//! still found (0038, "The session sweep").
//!
//! Each launcher (one per launch, R17) makes a [`SessionMark`] before its launch: a host-owned
//! file and a canary beside it, in a folder no profile can write. Its profile gets one rule,
//! `file-write-data` on the mark. A process belongs to the launch when the kernel answers, for
//! that process, that the mark may be written and the canary may not (`sandbox_check`, a private
//! but long-stable call looked up at run time, always with `SANDBOX_CHECK_NO_REPORT` so the scan
//! never floods the denial watcher). The canary is what keeps Apple's own sandboxed daemons out:
//! some may write any file in a folder, so they pass the mark too, and the canary too.
//!
//! One scan ([`processes`]) feeds three callers: the process cap's 2 s sweep
//! (`process_cap/watchdog.rs`, through [`LaunchSweep`], which holds only its launch's mark), the
//! mode-switch gate ([`switch_to_read_only`]) and session end ([`end_session`],
//! [`JailedSession`]). Every scan is vouched for by the launch's sentinel, a process under the
//! launch's own profile that must match (else [`SweepError::Unknown`]), and checks the private
//! call against the live agent too (else [`SweepError::Drift`]): never an empty list on a check
//! that stopped answering as measured.

use std::collections::HashSet;
use std::ffi::CStr;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clauseo_process::TreeWatch;

use super::process_cap::Proc;
use super::process_cap::libproc::Libproc;
use super::process_cap::watchdog::System as _;
use super::profile::TAG_PREFIX;

mod launch;
mod mark;
mod seatbelt;
mod sentinel;

pub(crate) use launch::LaunchSweep;
pub use mark::SessionMark;
pub(crate) use mark::check_folder;
pub(crate) use seatbelt::api;

/// How long a launch's agent may run before the drift check expects it to match its mark:
/// `sandbox-exec` applies the profile and execs in place within milliseconds of the spawn.
const ROOT_GRACE: Duration = Duration::from_secs(1);

/// The pause between two rounds of [`end_session_processes`], while killed processes exit.
const END_PAUSE: Duration = Duration::from_millis(5);

/// One process: its pid and its start, which together name it (a reused pid starts later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcId {
    /// Its pid.
    pid: u32,
    /// Its start, in microseconds since the epoch (`pbi_start_tvsec` and `pbi_start_tvusec`).
    start: u64,
}

impl ProcId {
    /// The process `pid` that started `start` microseconds after the epoch.
    pub(crate) fn new(pid: u32, start: u64) -> ProcId {
        ProcId { pid, start }
    }

    /// Its pid.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// When it started.
    pub fn start(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_micros(self.start)
    }

    fn of(process: &Proc) -> ProcId {
        ProcId::new(process.pid, process.start)
    }
}

/// Why a scan or an end could not say the session is gone.
#[derive(Debug, thiserror::Error)]
pub enum SweepError {
    /// A mark or its canary is missing or not a regular file. The kernel answers "denied" for a
    /// path that does not exist, for every process, so a scan would silently find nothing.
    #[error("the session mark {path} is unusable: {source}")]
    Mark {
        /// The mark or the canary.
        path: std::path::PathBuf,
        /// What `lstat` said, or why the file is refused.
        source: std::io::Error,
    },
    /// The process listing failed.
    #[error("the process listing failed")]
    Listing,
    /// The drift check: the agent of launch `tag`, alive and past its start, does not match its
    /// own mark. The private `sandbox_check` no longer answers as measured, and no scan can be
    /// trusted to be complete.
    #[error(
        "the session sweep's check failed: launch {tag}'s agent (pid {pid}) does not match its own mark"
    )]
    Drift {
        /// The launch's tag.
        tag: String,
        /// The agent's pid.
        pid: u32,
    },
    /// The scan cannot vouch for its answer: the sentinel of launch `tag`, a process under
    /// that launch's own profile, is missing or does not match the mark. The kernel's answers
    /// may have changed (a macOS update), so "nothing found" would mean nothing: the answer is
    /// unknown, never [`Gone`].
    #[error(
        "the session sweep cannot vouch for its scan: launch {tag}'s sentinel is not running or does not match its mark"
    )]
    Unknown {
        /// The launch's tag.
        tag: String,
    },
    /// The launch's sentinel could not start.
    #[error("the session sweep's sentinel could not start: {source}")]
    Sentinel {
        /// What the spawn said.
        source: std::io::Error,
    },
    /// This macOS lacks `sandbox_check` or `SANDBOX_CHECK_NO_REPORT` (looked up at run time):
    /// no scan can run, so no jailed launch may.
    #[error("this Mac's sandbox lacks the call the session sweep needs (sandbox_check)")]
    Unavailable,
    /// The deadline passed with processes of the session still running.
    #[error("the deadline passed with processes of the session running")]
    Deadline,
}

/// Every process of the session ended: a scan found none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct Gone {
    /// How many processes this end killed (zero when there were none).
    pub ended: usize,
}

/// The session could not be shown gone: what a scan still found, and why it stopped.
#[derive(Debug, thiserror::Error)]
#[error("{} process(es) of the session left: {why}", left.len())]
pub struct Survivors {
    /// The processes the last scan found (empty when the scan itself failed).
    pub left: Vec<ProcId>,
    /// Why the end gave up.
    #[source]
    pub why: SweepError,
}

/// The mode-switch gate's answer that the session may go read-only: only made from [`Gone`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct ReadOnly {
    gone: Gone,
}

impl ReadOnly {
    /// The end that made it.
    pub fn gone(self) -> Gone {
        self.gone
    }
}

/// The session handle of one launch ([`SeatbeltLauncher::launch_jailed`]): the agent's tree
/// (its [`TreeWatch`]) and the launch's [`SessionMark`]. A launcher makes one launch (R17), so
/// the mark, its sentinel and everything it finds are this launch's alone.
///
/// [`SeatbeltLauncher::launch_jailed`]: crate::SeatbeltLauncher::launch_jailed
#[derive(Debug, Clone)]
pub struct JailedSession {
    tree: TreeWatch,
    mark: Arc<SessionMark>,
    root: ProcId,
}

impl JailedSession {
    pub(crate) fn new(tree: TreeWatch, mark: Arc<SessionMark>, root: ProcId) -> JailedSession {
        JailedSession { tree, mark, root }
    }

    /// The agent's tree.
    pub fn tree(&self) -> &TreeWatch {
        &self.tree
    }

    /// The launch's mark.
    pub fn mark(&self) -> &Arc<SessionMark> {
        &self.mark
    }

    /// The agent: its pid and start.
    pub fn root(&self) -> ProcId {
        self.root
    }

    /// Whether the session is gone: the tree is gone, and [`processes`] with no start filter
    /// finds nothing, its sentinel vouching for the scan. A failed or unvouched scan answers
    /// `false`.
    pub fn processes_gone(&self) -> bool {
        self.tree.gone() && processes(&[&self.mark], UNIX_EPOCH).is_ok_and(|p| p.is_empty())
    }

    /// Ends every process of the session ([`end_session_processes`] on its mark). The
    /// sentinel stays: [`end_session`] ends it.
    pub fn end(&self, deadline: Instant) -> Result<Gone, Survivors> {
        end_session_processes(&[&self.mark], deadline)
    }
}

/// What the scan asks the kernel; [`seatbelt::Seatbelt`] in use, a fake in the unit tests.
pub(crate) trait Checker {
    /// Whether `pid` runs under any sandbox; `None` when the call failed (the process is gone).
    fn sandboxed(&self, pid: u32) -> Option<bool>;
    /// Whether `pid` may `file-write-data` the canonical, existing `path`; `None` on failure.
    fn may_write(&self, pid: u32, path: &CStr) -> Option<bool>;
}

/// Whether sandboxed process `pid` belongs to `mark`: the mark allowed and the canary denied.
fn matches(check: &dyn Checker, pid: u32, mark: &SessionMark) -> bool {
    check.may_write(pid, mark.mark_c()) == Some(true)
        && check.may_write(pid, mark.canary_c()) == Some(false)
}

/// The processes among `all` that belong to any of `marks`: this user's (`uid`), started at or
/// after `since` (microseconds since the epoch), sandboxed, the mark allowed and the canary
/// denied; never a mark's sentinel.
///
/// Each mark's sentinel must be among `all` and match its mark, whatever its start, else
/// [`SweepError::Unknown`]: the positive control that holds with no agent left. Each mark's
/// live agent is checked whatever its start; one that started more than [`ROOT_GRACE`] before
/// `now` and does not match its own mark is [`SweepError::Drift`].
pub(crate) fn scan(
    check: &dyn Checker,
    marks: &[&SessionMark],
    since: u64,
    now: u64,
    uid: u32,
    all: &[Proc],
) -> Result<Vec<ProcId>, SweepError> {
    let mut sentinels = Vec::with_capacity(marks.len());
    for mark in marks {
        mark.check_files()?;
        sentinels.push(mark.sentinel().ok_or_else(|| SweepError::Unknown {
            tag: mark.tag().to_owned(),
        })?);
    }
    let mut vouched = vec![false; marks.len()];
    let grace = u64::try_from(ROOT_GRACE.as_micros()).unwrap_or(u64::MAX);
    let mut found = Vec::new();
    for process in all.iter().filter(|p| p.uid == uid) {
        let id = ProcId::of(process);
        let sentinel = sentinels.iter().position(|s| *s == id);
        let own = marks.iter().position(|m| m.root() == Some(id));
        if process.start < since && own.is_none() && sentinel.is_none() {
            continue;
        }
        let sandboxed = check.sandboxed(process.pid) == Some(true);
        let hits: Vec<usize> = if sandboxed {
            (0..marks.len())
                .filter(|&i| marks.get(i).is_some_and(|m| matches(check, process.pid, m)))
                .collect()
        } else {
            Vec::new()
        };
        if let Some(i) = sentinel {
            // The control: never a process of the session, only proof the answers hold.
            if hits.contains(&i)
                && let Some(seen) = vouched.get_mut(i)
            {
                *seen = true;
            }
            continue;
        }
        if !hits.is_empty() {
            found.push(id);
        }
        let settled = process.start.saturating_add(grace) <= now;
        if let Some(i) = own.filter(|i| settled && !hits.contains(i)) {
            let tag = marks.get(i).map(|m| m.tag().to_owned()).unwrap_or_default();
            return Err(SweepError::Drift {
                tag,
                pid: process.pid,
            });
        }
    }
    if let Some(i) = vouched.iter().position(|seen| !seen) {
        let tag = marks.get(i).map(|m| m.tag().to_owned()).unwrap_or_default();
        return Err(SweepError::Unknown { tag });
    }
    Ok(found)
}

/// Every running process of the sessions `marks` name that started at or after `since`, the
/// launches' agents among them, their sentinels never. Each call checks the marks exist, that
/// each sentinel matches and runs the drift check.
pub fn processes(marks: &[&SessionMark], since: SystemTime) -> Result<Vec<ProcId>, SweepError> {
    processes_with(api()?, marks, since)
}

/// [`processes`] through `check`.
fn processes_with(
    check: &dyn Checker,
    marks: &[&SessionMark],
    since: SystemTime,
) -> Result<Vec<ProcId>, SweepError> {
    let all = all().ok_or(SweepError::Listing)?;
    scan(
        check,
        marks,
        micros(since),
        micros(now_wall()),
        seatbelt::uid(),
        &all,
    )
}

/// Ends every process of the sessions `marks` name, the launches' agents included, by
/// [`end_session_processes`]; then, only on [`Gone`], ends each sentinel and removes the
/// marks ([`SessionMark::remove`]). A mark that cannot be removed is logged: the session is
/// gone.
pub fn end_session(marks: &[&SessionMark], deadline: Instant) -> Result<Gone, Survivors> {
    let gone = end_session_processes(marks, deadline)?;
    for mark in marks {
        if let Err(error) = mark.finish() {
            tracing::warn!(tag = mark.tag(), %error, "a session mark could not be removed");
        }
    }
    Ok(gone)
}

/// The mode-switch gate: ends every process of the matter's sessions (`marks`, one per launch)
/// and answers [`ReadOnly`] only on [`Gone`]. On [`Survivors`] the switch fails closed, an
/// unvouched scan ([`SweepError::Unknown`]) among them.
pub fn switch_to_read_only(
    marks: &[&SessionMark],
    deadline: Instant,
) -> Result<ReadOnly, Survivors> {
    match api() {
        Ok(check) => gate_with(check, marks, deadline),
        Err(why) => Err(Survivors {
            left: kill_roots(marks),
            why,
        }),
    }
}

/// [`switch_to_read_only`] through `check`.
fn gate_with(
    check: &dyn Checker,
    marks: &[&SessionMark],
    deadline: Instant,
) -> Result<ReadOnly, Survivors> {
    end_with(check, marks, deadline).map(|gone| ReadOnly { gone })
}

/// Stops, then kills, every process of the sessions `marks` name, until a scan finds none
/// ([`Gone`]) or `deadline` passes ([`Survivors`], [`SweepError::Deadline`]). A scan that fails,
/// an unvouched one or the drift check's above all, ends it at once ([`Survivors`] with that
/// error): it never answers [`Gone`] on a scan it cannot trust. The sentinels are never
/// signalled.
///
/// Each launch's agent leads its own session, so one `killpg(SIGSTOP)` freezes its group first.
/// Then each round scans: every process found that is not stopped yet gets `SIGSTOP` (its start
/// checked first), and the round repeats, since a stopped process cannot fork; once a round
/// finds nothing new, each one found gets `SIGKILL`, its start checked again. The scans have no
/// start filter (about 1.3 ms each rather than 0.2): process starts are wall-clock stamps, and a
/// clock set back must not hide a process from the gate.
pub fn end_session_processes(marks: &[&SessionMark], deadline: Instant) -> Result<Gone, Survivors> {
    match api() {
        Ok(check) => end_with(check, marks, deadline),
        Err(why) => Err(Survivors {
            left: kill_roots(marks),
            why,
        }),
    }
}

/// The launches' agents that still run with their starts.
fn live_roots(marks: &[&SessionMark]) -> Vec<ProcId> {
    marks
        .iter()
        .filter_map(|mark| mark.root())
        .filter(|root| start_of(root.pid) == Some(root.start))
        .collect()
}

/// Kills the launches' agents that still run (by their starts); answers them.
fn kill_roots(marks: &[&SessionMark]) -> Vec<ProcId> {
    let left = live_roots(marks);
    for root in &left {
        Libproc.signal(root.pid, root.start, libc::SIGKILL);
    }
    left
}

/// [`end_session_processes`] through `check`.
fn end_with(
    check: &dyn Checker,
    marks: &[&SessionMark],
    deadline: Instant,
) -> Result<Gone, Survivors> {
    for root in live_roots(marks) {
        Libproc.signal_group(root.pid, libc::SIGSTOP);
    }
    let mut stopped: HashSet<ProcId> = HashSet::new();
    let mut ended: HashSet<ProcId> = HashSet::new();
    loop {
        let mut targets = match processes_with(check, marks, UNIX_EPOCH) {
            Ok(found) => found,
            // The scan cannot be trusted; the agents themselves are known, so they go.
            Err(why) => {
                return Err(Survivors {
                    left: kill_roots(marks),
                    why,
                });
            }
        };
        // An agent still starting may not carry its profile yet; it is ended all the same.
        for root in live_roots(marks) {
            if !targets.contains(&root) {
                targets.push(root);
            }
        }
        if targets.is_empty() {
            return Ok(Gone { ended: ended.len() });
        }
        if now() >= deadline {
            for id in &targets {
                Libproc.signal(id.pid, id.start, libc::SIGKILL);
            }
            return Err(Survivors {
                left: targets,
                why: SweepError::Deadline,
            });
        }
        let fresh: Vec<ProcId> = targets
            .iter()
            .copied()
            .filter(|id| !stopped.contains(id))
            .collect();
        if fresh.is_empty() {
            for id in &targets {
                if Libproc.signal(id.pid, id.start, libc::SIGKILL) {
                    ended.insert(*id);
                }
            }
            thread::park_timeout(END_PAUSE);
        } else {
            for id in fresh {
                Libproc.signal(id.pid, id.start, libc::SIGSTOP);
                stopped.insert(id);
            }
        }
    }
}

/// Every process this user can read, zombies left out; `None` when the listing fails.
fn all() -> Option<Vec<Proc>> {
    Some(
        Libproc
            .pids()?
            .into_iter()
            .filter_map(|pid| Libproc.info(pid))
            .collect(),
    )
}

/// The start of running process `pid`; `None` when it is gone, unreadable or a zombie.
pub(crate) fn start_of(pid: u32) -> Option<u64> {
    Libproc.info(pid).map(|process| process.start)
}

/// Removes the stale marks in `dir` (canonical), the marks folder: those of launchers of an
/// earlier host, whose tag names a host pid that is not this process's and runs no longer, and
/// that no process matches ([`SessionMark::remove_if_unused`]). A mark a process still
/// matches, one whose scan fails, or one of a host that still runs is never touched. Answers how
/// many went.
pub(crate) fn clean_stale(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let Some(host) = entry.file_name().to_str().and_then(host_of) else {
            continue;
        };
        if host == std::process::id() || host_alive(host) {
            continue;
        }
        if let Ok(mark) = SessionMark::open(&entry.path())
            && mark.remove_if_unused()
        {
            removed += 1;
        }
    }
    removed
}

/// The host pid a launcher's tag names: `clauseo-<pid>-<n>-<nonce>`.
fn host_of(tag: &str) -> Option<u32> {
    tag.strip_prefix(TAG_PREFIX)?
        .split('-')
        .next()?
        .parse()
        .ok()
}

/// Whether `pid` names a running process (another host may still hold its marks).
fn host_alive(pid: u32) -> bool {
    Libproc.info(pid).is_some() || Libproc.session_of(pid).is_some()
}

/// Microseconds since the epoch; 0 before it.
fn micros(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

/// The wall clock the process starts are compared with.
#[expect(
    clippy::disallowed_methods,
    reason = "the sweep compares process starts, wall-clock stamps the kernel keeps, with now"
)]
fn now_wall() -> SystemTime {
    SystemTime::now()
}

/// The monotonic clock the end's deadline runs on.
#[expect(
    clippy::disallowed_methods,
    reason = "the end's deadline: a real-time loop against live processes, not core logic"
)]
fn now() -> Instant {
    Instant::now()
}

#[cfg(test)]
mod tests;
