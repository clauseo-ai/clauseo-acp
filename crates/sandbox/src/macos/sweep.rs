//! The session sweep: finding and ending every process of a jailed launch, wherever it went: one
//! that called `setsid`, double-forked, closed every descriptor and was adopted by launchd is
//! still found (0038, "The session sweep").
//!
//! Each launcher makes a [`SessionMark`] before its first launch: a host-owned file and a canary
//! beside it, in a folder no profile can write. Its profile gets one rule, `file-write-data` on
//! the mark. A process belongs to the launch when the kernel answers, for that process, that the
//! mark may be written and the canary may not (`sandbox_check`, a private but long-stable call,
//! always with `SANDBOX_CHECK_NO_REPORT` so the scan never floods the denial watcher). The canary
//! is what keeps Apple's own sandboxed daemons out: some may write any file in a folder, so they
//! pass the mark too, and the canary too.
//!
//! One scan ([`processes`]) feeds three callers: the process cap's 2 s sweep
//! (`process_cap/watchdog.rs`, through `escaped`), the mode-switch gate
//! ([`switch_to_read_only`]) and session end ([`end_session`], [`JailedSession`]). Every scan
//! checks the private call against the agent itself: a live agent that does not match its own
//! mark is [`SweepError::Drift`], never an empty list.

use std::collections::HashSet;
use std::ffi::CStr;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clauseo_process::TreeWatch;

use super::process_cap::Proc;
use super::process_cap::libproc::Libproc;
use super::process_cap::watchdog::System as _;

mod mark;
mod seatbelt;

pub use mark::SessionMark;

/// How long a launch's agent may run before the drift check expects it to match its mark:
/// `sandbox-exec` applies the profile and execs in place within milliseconds of the spawn.
const ROOT_GRACE: Duration = Duration::from_secs(1);

/// The pause between two rounds of [`end_session_processes`], while killed processes exit.
const END_PAUSE: Duration = Duration::from_millis(5);

/// How many rounds [`take_over`] runs before it leaves the rest to the watchdog's next sweep.
const TAKE_OVER_ROUNDS: usize = 20;

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
/// (its [`TreeWatch`]) and the launcher's [`SessionMark`]. The mark is the launcher's, shared by
/// all its launches, so a session here is everything the launcher started.
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

    /// The launcher's mark.
    pub fn mark(&self) -> &Arc<SessionMark> {
        &self.mark
    }

    /// The agent: its pid and start.
    pub fn root(&self) -> ProcId {
        self.root
    }

    /// Whether the session is gone: the tree is gone, and [`processes`] with no start filter
    /// finds nothing with the drift check passing. A failed scan answers `false`.
    pub fn processes_gone(&self) -> bool {
        self.tree.gone() && processes(&[&self.mark], UNIX_EPOCH).is_ok_and(|p| p.is_empty())
    }

    /// Ends every process of the session ([`end_session_processes`] on its mark).
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
/// denied. Each mark's live agents are checked whatever their start; one that started more than
/// [`ROOT_GRACE`] before `now` and does not match its own mark is [`SweepError::Drift`].
pub(crate) fn scan(
    check: &dyn Checker,
    marks: &[&SessionMark],
    since: u64,
    now: u64,
    uid: u32,
    all: &[Proc],
) -> Result<Vec<ProcId>, SweepError> {
    for mark in marks {
        mark.check_files()?;
    }
    let roots: Vec<(usize, ProcId)> = marks
        .iter()
        .enumerate()
        .flat_map(|(i, mark)| mark.roots().into_iter().map(move |root| (i, root)))
        .collect();
    let grace = u64::try_from(ROOT_GRACE.as_micros()).unwrap_or(u64::MAX);
    let mut found = Vec::new();
    for process in all.iter().filter(|p| p.uid == uid) {
        let id = ProcId::of(process);
        let own_roots: Vec<usize> = roots
            .iter()
            .filter(|(_, root)| *root == id)
            .map(|(i, _)| *i)
            .collect();
        if process.start < since && own_roots.is_empty() {
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
        if !hits.is_empty() {
            found.push(id);
        }
        let settled = process.start.saturating_add(grace) <= now;
        if let Some(&i) = own_roots.iter().find(|i| settled && !hits.contains(i)) {
            let tag = marks.get(i).map(|m| m.tag().to_owned()).unwrap_or_default();
            return Err(SweepError::Drift {
                tag,
                pid: process.pid,
            });
        }
    }
    Ok(found)
}

/// Every running process of the sessions `marks` name that started at or after `since`, the
/// launches' agents among them. Each call checks the marks exist and runs the drift check.
pub fn processes(marks: &[&SessionMark], since: SystemTime) -> Result<Vec<ProcId>, SweepError> {
    let all = all().ok_or(SweepError::Listing)?;
    scan(
        &seatbelt::Seatbelt,
        marks,
        micros(since),
        micros(now_wall()),
        seatbelt::uid(),
        &all,
    )
}

/// Ends every process of the sessions `marks` name, the launches' agents included, by
/// [`end_session_processes`]; then, only on [`Gone`], removes the marks
/// ([`SessionMark::remove`]). A mark that cannot be removed is logged: the session is gone.
pub fn end_session(marks: &[&SessionMark], deadline: Instant) -> Result<Gone, Survivors> {
    let gone = end_session_processes(marks, deadline)?;
    for mark in marks {
        if let Err(error) = mark.remove() {
            tracing::warn!(tag = mark.tag(), %error, "a session mark could not be removed");
        }
    }
    Ok(gone)
}

/// The mode-switch gate: ends every process of the matter's sessions (`marks`, one per launch)
/// and answers [`ReadOnly`] only on [`Gone`]. On [`Survivors`] the switch fails closed.
pub fn switch_to_read_only(
    marks: &[&SessionMark],
    deadline: Instant,
) -> Result<ReadOnly, Survivors> {
    end_session_processes(marks, deadline).map(|gone| ReadOnly { gone })
}

/// Stops, then kills, every process of the sessions `marks` name, until a scan finds none
/// ([`Gone`]) or `deadline` passes ([`Survivors`], [`SweepError::Deadline`]). A scan that fails,
/// the drift check's above all, ends it at once ([`Survivors`] with that error): it never
/// answers [`Gone`] on a scan it cannot trust.
///
/// Each launch's agent leads its own session, so one `killpg(SIGSTOP)` freezes its group first.
/// Then each round scans: every process found that is not stopped yet gets `SIGSTOP` (its start
/// checked first), and the round repeats, since a stopped process cannot fork; once a round
/// finds nothing new, each one found gets `SIGKILL`, its start checked again. The scans have no
/// start filter (about 1.3 ms each rather than 0.2): process starts are wall-clock stamps, and a
/// clock set back must not hide a process from the gate.
pub fn end_session_processes(marks: &[&SessionMark], deadline: Instant) -> Result<Gone, Survivors> {
    let since = UNIX_EPOCH;
    let roots = || -> Vec<ProcId> {
        marks
            .iter()
            .flat_map(|mark| mark.roots())
            .filter(|root| start_of(root.pid) == Some(root.start))
            .collect()
    };
    for root in roots() {
        Libproc.signal_group(root.pid, libc::SIGSTOP);
    }
    let mut stopped: HashSet<ProcId> = HashSet::new();
    let mut ended: HashSet<ProcId> = HashSet::new();
    loop {
        let mut targets = match processes(marks, since) {
            Ok(found) => found,
            Err(why) => {
                // The scan cannot be trusted; the agents themselves are known, so they go.
                let left = roots();
                for root in &left {
                    Libproc.signal(root.pid, root.start, libc::SIGKILL);
                }
                return Err(Survivors { left, why });
            }
        };
        // An agent still starting may not carry its profile yet; it is ended all the same.
        for root in roots() {
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

/// The marks of the launches whose agents the watchdog follows, by agent.
static ROOTS: Mutex<Vec<(ProcId, Weak<SessionMark>)>> = Mutex::new(Vec::new());

/// Records `root` as an agent of `mark`'s launcher: its drift check, and the watchdog's
/// [`escaped`].
pub(crate) fn register(mark: &Arc<SessionMark>, root: ProcId) {
    mark.add_root(root);
    let mut roots = ROOTS.lock().unwrap_or_else(PoisonError::into_inner);
    roots.retain(|(id, weak)| weak.strong_count() > 0 && start_of(id.pid) == Some(id.start));
    roots.push((root, Arc::downgrade(mark)));
}

/// The process cap's sweep: the processes among `all` of the launch whose agent is `root`
/// (started `start`), started since the agent, the agent left out: those that left its tree
/// and group are found by its mark. Empty when the agent has no mark. A scan error is logged
/// at error level once per agent and answers nothing more than the tree walk found.
pub(crate) fn escaped(root: u32, start: u64, all: &[Proc]) -> Vec<Proc> {
    let Some(mark) = mark_of(|r| *r == ProcId::new(root, start)) else {
        return Vec::new();
    };
    let now = micros(now_wall());
    match scan(
        &seatbelt::Seatbelt,
        &[&mark],
        start,
        now,
        seatbelt::uid(),
        all,
    ) {
        Ok(found) => all
            .iter()
            .filter(|p| p.pid != root && found.contains(&ProcId::of(p)))
            .copied()
            .collect(),
        Err(error) => {
            if mark.tell_failure() {
                tracing::error!(%error, "the process cap's session sweep failed");
            }
            Vec::new()
        }
    }
}

/// Adds to `found`, the agent's descendants by session and parent, the [`escaped`] ones it
/// does not hold: the watchdog's one call into the sweep. Only the processes `found` does not
/// hold are asked about, so a bomb in the session costs no kernel query at a trip.
pub(crate) fn add_escaped(root: u32, start: u64, all: &[Proc], found: &mut Vec<Proc>) {
    let known: HashSet<u32> = found.iter().map(|p| p.pid).collect();
    let rest: Vec<Proc> = all
        .iter()
        .filter(|p| !known.contains(&p.pid))
        .copied()
        .collect();
    found.extend(escaped(root, start, &rest));
}

/// The process cap's hand-over: what a trip's kill rounds left running (`left`), and every
/// process of agent `root`'s launch the mark still finds (the agent left out), stopped and then
/// killed, each by its start, for up to [`TAKE_OVER_ROUNDS`] rounds. Answers how many still
/// run; the watchdog's next sweep counts them again.
pub(crate) fn take_over(root: u32, left: &[Proc]) -> usize {
    let entry = {
        let roots = ROOTS.lock().unwrap_or_else(PoisonError::into_inner);
        roots
            .iter()
            .rev()
            .find(|(id, _)| id.pid == root)
            .and_then(|(id, weak)| Some((*id, weak.upgrade()?)))
    };
    let mut targets: Vec<ProcId> = left.iter().map(ProcId::of).collect();
    for _ in 0..TAKE_OVER_ROUNDS {
        if let Some((agent, mark)) = &entry
            && let Some(all) = all()
            && let Ok(found) = scan(
                &seatbelt::Seatbelt,
                &[mark],
                agent.start,
                micros(now_wall()),
                seatbelt::uid(),
                &all,
            )
        {
            for id in found {
                if id != *agent && !targets.contains(&id) {
                    targets.push(id);
                }
            }
        }
        targets.retain(|id| start_of(id.pid) == Some(id.start));
        if targets.is_empty() {
            return 0;
        }
        for id in &targets {
            Libproc.signal(id.pid, id.start, libc::SIGSTOP);
        }
        for id in &targets {
            Libproc.signal(id.pid, id.start, libc::SIGKILL);
        }
        thread::park_timeout(END_PAUSE);
    }
    targets.retain(|id| start_of(id.pid) == Some(id.start));
    targets.len()
}

/// The mark of the first registered agent `which` accepts, if its launcher still holds it.
fn mark_of(which: impl Fn(&ProcId) -> bool) -> Option<Arc<SessionMark>> {
    let roots = ROOTS.lock().unwrap_or_else(PoisonError::into_inner);
    roots
        .iter()
        .find(|(id, _)| which(id))
        .and_then(|(_, weak)| weak.upgrade())
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
