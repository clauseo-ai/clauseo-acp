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
//! launch's own profile that must match (else [`SweepError::Unknown`], or
//! [`SweepError::SentinelGone`] when it was killed), and checks the private call against the live
//! agent too (else [`SweepError::Drift`]): never an empty list on a check that stopped answering
//! as measured. An unvouched scan still answers what it found: a match is positive evidence, so
//! every caller ends it all the same, and only "nothing else runs" is withheld. A dead sentinel is
//! restarted ([`SessionMark::revive`]); the process cap's watchdog ends the session when one is
//! killed mid-session, since only tampering explains that.

use std::ffi::CStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clauseo_process::TreeWatch;

use super::process_cap::Proc;
use super::process_cap::libproc::Libproc;
use super::process_cap::watchdog::System as _;

mod end;
mod launch;
mod mark;
mod seatbelt;
mod sentinel;
mod stale;

#[cfg(test)]
use end::gate_with;
pub use end::{end_session, end_session_processes, switch_to_read_only};
pub(crate) use launch::LaunchSweep;
pub use mark::SessionMark;
pub(crate) use mark::check_folder;
pub(crate) use seatbelt::api;
pub(crate) use stale::clean_stale;
#[cfg(test)]
use stale::host_of;

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
    /// path that does not exist, for every process, so a scan would silently find nothing. It
    /// names the file by its role, never its path (launch errors carry it).
    #[error("the session's {file} file is unusable ({kind})")]
    Mark {
        /// Which file: `mark` or `canary`.
        file: &'static str,
        /// What `lstat` said (`InvalidData`: not a regular file).
        kind: std::io::ErrorKind,
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
        "the session sweep cannot vouch for its scan: launch {tag}'s sentinel is not running or does not match its mark, or the check failed"
    )]
    Unknown {
        /// The launch's tag.
        tag: String,
    },
    /// The launch's sentinel was killed: started, never ended by this process, and no longer
    /// running. The scan is unknown, as for [`SweepError::Unknown`]; mid-session only tampering
    /// explains it, so the watchdog ends the session, and the sentinel is restarted.
    #[error("the session sweep cannot vouch for its scan: launch {tag}'s sentinel was killed")]
    SentinelGone {
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
    #[error("this Mac's sandbox lacks a call the session sweep needs")]
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

/// Why one check failed (`sandbox_check` answered -1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckFailed {
    /// `ESRCH`: the process exited since the listing.
    Gone,
    /// Any other `errno`: the call no longer answers, so no scan is complete.
    Failed,
}

/// One answer of the kernel's check.
pub(crate) type Answer = Result<bool, CheckFailed>;

/// What the scan asks the kernel; [`seatbelt::Seatbelt`] in use, a fake in the unit tests.
pub(crate) trait Checker {
    /// Whether `pid` runs under any sandbox.
    fn sandboxed(&self, pid: u32) -> Answer;
    /// Whether `pid` may `file-write-data` the canonical, existing `path`.
    fn may_write(&self, pid: u32, path: &CStr) -> Answer;
}

/// Whether sandboxed process `pid` belongs to `mark`: the mark allowed and the canary denied.
/// A process gone since the listing does not; a failed call is [`CheckFailed::Failed`].
fn matches(check: &dyn Checker, pid: u32, mark: &SessionMark) -> Result<bool, CheckFailed> {
    let answers = (
        check.may_write(pid, mark.mark_c()),
        check.may_write(pid, mark.canary_c()),
    );
    match answers {
        (Ok(true), Ok(false)) => Ok(true),
        (Err(CheckFailed::Failed), _) | (_, Err(CheckFailed::Failed)) => Err(CheckFailed::Failed),
        _ => Ok(false),
    }
}

/// What one scan found, and why it cannot say nothing else runs.
#[derive(Debug)]
pub(crate) struct Scan {
    /// Every process found matching a mark: positive evidence even when the scan is unvouched.
    pub(crate) found: Vec<ProcId>,
    /// `None` when every sentinel vouched for the scan; else why it is incomplete
    /// ([`SweepError::SentinelGone`], [`SweepError::Drift`] or [`SweepError::Unknown`]).
    pub(crate) doubt: Option<SweepError>,
}

impl Scan {
    /// What it found, if the scan was vouched for; else its doubt.
    pub(crate) fn vouched(self) -> Result<Vec<ProcId>, SweepError> {
        match self.doubt {
            None => Ok(self.found),
            Some(doubt) => Err(doubt),
        }
    }
}

/// The processes among `all` that belong to any of `marks`: this user's (`uid`), started at or
/// after `since` (microseconds since the epoch), sandboxed, the mark allowed and the canary
/// denied; never a mark's sentinel.
///
/// Each mark's sentinel must be among `all` and match its mark, whatever its start: the positive
/// control that holds with no agent left. A sentinel started and not ended that is no longer
/// listed was killed ([`SweepError::SentinelGone`]); one missing or not matching, or a call that
/// failed for any process but one gone since the listing, is [`SweepError::Unknown`]. Each
/// mark's live agent is checked whatever its start; one that started more than [`ROOT_GRACE`]
/// before `now` and does not match its own mark is [`SweepError::Drift`]. Each of these is the
/// scan's doubt: what it found stands. A mark whose files are unusable fails the scan.
pub(crate) fn scan(
    check: &dyn Checker,
    marks: &[&SessionMark],
    since: u64,
    now: u64,
    uid: u32,
    all: &[Proc],
) -> Result<Scan, SweepError> {
    let mut sentinels = Vec::with_capacity(marks.len());
    for mark in marks {
        mark.check_files()?;
        sentinels.push(mark.sentinel());
    }
    let mut listed = vec![false; marks.len()];
    let mut vouched = vec![false; marks.len()];
    let mut failed = false;
    let mut drift = None;
    let grace = u64::try_from(ROOT_GRACE.as_micros()).unwrap_or(u64::MAX);
    let mut found = Vec::new();
    for process in all.iter().filter(|p| p.uid == uid) {
        let id = ProcId::of(process);
        let sentinel_of: Vec<usize> = (0..marks.len())
            .filter(|&i| sentinels.get(i).copied().flatten() == Some(id))
            .collect();
        let own = marks.iter().position(|m| m.root() == Some(id));
        if process.start < since && own.is_none() && sentinel_of.is_empty() {
            continue;
        }
        let sandboxed = match check.sandboxed(process.pid) {
            Ok(sandboxed) => sandboxed,
            Err(CheckFailed::Gone) => false,
            Err(CheckFailed::Failed) => {
                failed = true;
                false
            }
        };
        let mut hits = Vec::new();
        if sandboxed {
            for (i, mark) in marks.iter().enumerate() {
                match matches(check, process.pid, mark) {
                    Ok(true) => hits.push(i),
                    Ok(false) => {}
                    Err(_) => failed = true,
                }
            }
        }
        if !sentinel_of.is_empty() {
            // The control: never a process of the session, only proof the answers hold.
            for i in sentinel_of {
                if let Some(seen) = listed.get_mut(i) {
                    *seen = true;
                }
                if hits.contains(&i)
                    && let Some(seen) = vouched.get_mut(i)
                {
                    *seen = true;
                }
            }
            continue;
        }
        if !hits.is_empty() {
            found.push(id);
        }
        let settled = process.start.saturating_add(grace) <= now;
        if drift.is_none()
            && let Some(i) = own.filter(|i| settled && !hits.contains(i))
        {
            let tag = marks.get(i).map(|m| m.tag().to_owned()).unwrap_or_default();
            drift = Some(SweepError::Drift {
                tag,
                pid: process.pid,
            });
        }
    }
    let tag = |i: usize| marks.get(i).map(|m| m.tag().to_owned()).unwrap_or_default();
    let killed = (0..marks.len()).find(|&i| {
        sentinels.get(i).is_some_and(Option::is_some) && !listed.get(i).copied().unwrap_or(false)
    });
    let doubt = if let Some(i) = killed {
        Some(SweepError::SentinelGone { tag: tag(i) })
    } else if drift.is_some() {
        drift
    } else if let Some(i) = vouched.iter().position(|seen| !seen) {
        Some(SweepError::Unknown { tag: tag(i) })
    } else if failed {
        Some(SweepError::Unknown { tag: tag(0) })
    } else {
        None
    };
    Ok(Scan { found, doubt })
}

/// Every running process of the sessions `marks` name that started at or after `since`, the
/// launches' agents among them, their sentinels never. Each call checks the marks exist, that
/// each sentinel matches and runs the drift check.
pub fn processes(marks: &[&SessionMark], since: SystemTime) -> Result<Vec<ProcId>, SweepError> {
    scan_with(api()?, marks, since)?.vouched()
}

/// One [`scan`] of every process through `check`.
fn scan_with(
    check: &dyn Checker,
    marks: &[&SessionMark],
    since: SystemTime,
) -> Result<Scan, SweepError> {
    let all = all().ok_or(SweepError::Listing)?;
    let scan = scan(
        check,
        marks,
        micros(since),
        micros(now_wall()),
        seatbelt::uid(),
        &all,
    )?;
    Ok(settle(scan, marks))
}

/// `scan`, its doubt [`SweepError::SentinelGone`] when it was unknown because a sentinel was
/// being killed: listed alive, it no longer matched by the time it was asked about.
fn settle(mut scan: Scan, marks: &[&SessionMark]) -> Scan {
    if matches!(scan.doubt, Some(SweepError::Unknown { .. }))
        && let Some(mark) = marks.iter().find(|mark| mark.sentinel_dead())
    {
        scan.doubt = Some(SweepError::SentinelGone {
            tag: mark.tag().to_owned(),
        });
    }
    scan
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
