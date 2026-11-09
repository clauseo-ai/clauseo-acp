//! The sweep of the pool user's processes (M1): every process whose token user is the pool user
//! is killed, but the sweeping process itself and, mid-session, what is in this launch's
//! kill-on-close job. It runs as the pool user, which can list its own processes; the desktop
//! user can't (`WTSEnumerateProcessesW` lists another user's processes only to Administrators,
//! and the outer runs unelevated), so the outer sweeps nothing.
//!
//! A pool user serves one live session at a time (`pool::assign::busy`), so no process of it
//! outside this launch's job belongs to a live session. The sweeps, all in the inner runner:
//!
//! - [`Point::Start`]: the inner runner's first act once resumed (`inner_flow.rs`), before it
//!   reads the launch or makes anything. Only the inner itself is kept, never another process
//!   of the job: none should be in it yet (the job is unnamed, its only handle is the outer's,
//!   and the inner has started nothing), and one a process that opened the inner before the
//!   outer's shut started with the inner as its parent would be. The job's pid list is read all
//!   the same, so a query that doesn't work fails the launch before any agent runs. The first
//!   failure fails the launch closed (`RunExit::Escapee` for a process that can't be opened or
//!   ended, else `RunExit::Sweep`).
//! - [`Point::Tick`]: every [`TICK`] while the agent runs, keeping the job's processes: one some
//!   Windows service started outside the job lives at most that long.
//! - [`Point::End`]: once the agent exited, keeping the job's processes (the outer ends the job
//!   itself). What is left outside the job after that is the next session's first act's.
//!
//! A process whose kill is refused is looked at again for up to 2 s after the round's kills, and
//! one that exited by itself meanwhile is no failure (`refused.rs`). At every point but the
//! first, a process that can't be opened, checked or ended is logged and the sweep goes on with
//! the rest; it is tried again in the sweep's next round and at the next tick, and the sweep ends
//! in [`SweepError::Left`].
//!
//! **The job's processes** ([`job_kept`]). The inner holds no handle to its job: a
//! kill-on-close job ends its processes only when its last handle closes, and one held by the
//! inner would keep the inner and the agent alive past the outer. It asks for the pid list of its
//! own job instead (`JobObjectBasicProcessIdList` with no job handle, which names the caller's
//! job), once a round, after the round's processes are opened (each pid pinned). A process is
//! the job's when its pid is in that list and it was created no earlier than the inner: nothing
//! created before the inner can be in the inner's job, so such a process holds a reused pid. A
//! list that can't be read fails the sweep: the first act closed, a tick until the next.
//!
//! **Escapees.** A process of the pool user's that can't be opened (after the owner's
//! `WRITE_DAC` reopen, below) or can't be ended is an escapee ([`SweepError::escapees`]): it
//! lives on outside every job, and no sweep of the pool user's can end it. The inner reports it on
//! `ctl` (`InnerReport::Escapee`), and the first act exits `RunExit::Escapee` rather than
//! `RunExit::Sweep`. One that only can't be checked (its start time) is a plain failure.
//!
//! **Two listings.** [`Processes::list`] reads each process's user by opening it, which fails
//! for a process whose DACL shuts the pool user out (one that set its own, with an `OWNER
//! RIGHTS` entry so its owner keeps no implied `WRITE_DAC`). [`Processes::named`] gives the
//! user of each of the caller's own processes whatever their DACL (`WTSEnumerateProcessesW`,
//! whose service reads the tokens). A pid either names the pool user is the pool user's, so such
//! a process is found, fails its open, and fails the first act closed.
//!
//! **Rounds.** A sweep lists again after any round that killed something, until a round kills
//! nothing: a process one it killed started meanwhile is killed in the next round. A runner of an
//! earlier session's is open to a later inner's sweep: the runners' process DACL grants the
//! desktop user's logon SID, which both hold (`token::runner_process_sddl`).
//!
//! Each process found is opened first, which pins its pid, and then the lists are read again: only
//! a pid they still give the pool user is killed, so a pid reused meanwhile is left alone.
//!
//! A kill is a signalled handle, and a process is known by its pid and creation time
//! (`kill.rs`).
//!
//! The calls are behind [`Processes`]; Windows' is `os.rs`.

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::time::Duration;

use crate::windows::sids::Sid;

mod failures;
mod kill;
#[cfg(windows)]
pub(super) mod os;
mod refused;

use failures::Failures;
use kill::{Done, Killed};

/// How often the inner sweeps while the agent runs.
pub(crate) const TICK: Duration = Duration::from_secs(30);
/// How many rounds one sweep makes: a round that kills something is followed by another, for
/// what those processes started meanwhile.
const ROUNDS: usize = 8;
/// How long a killed process has to be gone (its handle signalled) before it is a survivor.
pub(crate) const KILL_WAIT: Duration = Duration::from_secs(5);
/// The most one sweep waits on processes' ends, every kill's and refusal's together: past it, no
/// wait is longer than none, and the sweep ends with what its held handles say.
pub(crate) const SWEEP_DEADLINE: Duration = Duration::from_secs(60);
/// The exit code a swept process ends with: the runner's `RunExit` range's last code.
pub(crate) const SWEPT: u32 = super::RUN_EXIT_BASE + 0xff;
/// The most image names an escapee report carries.
pub(crate) const IMAGES_CAP: usize = 8;

/// Where the sweep runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Point {
    /// The inner runner's first act, before it reads the launch.
    Start,
    /// While the agent runs, every [`TICK`].
    Tick,
    /// In the inner, once the agent exited.
    End,
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Point::Start => "before launch",
            Point::Tick => "mid-session",
            Point::End => "session end",
        })
    }
}

/// Whose process it is, as the list gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Owner {
    /// Its token's user.
    User(Sid),
    /// No user this process can read: it can't open the process with
    /// `PROCESS_QUERY_LIMITED_INFORMATION`, or its token with `TOKEN_QUERY` (another user's
    /// process, a protected one, the kernel's own, or one that shut its DACL to its user); or,
    /// in [`Processes::named`], no SID (the kernel's and protected processes). Passed over
    /// unless the other listing names a user.
    None,
}

/// One process in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) pid: u32,
    /// Its image's file name, for the log.
    pub(crate) image: String,
    pub(crate) owner: Owner,
}

/// The system calls a sweep makes.
pub(crate) trait Processes {
    /// An open process; holding it pins its pid.
    type Process;
    /// Every process on the machine, with its user as far as this process can read it by
    /// opening it.
    fn list(&mut self) -> io::Result<Vec<Seen>>;
    /// The processes a service names with their user, whatever their DACL: this user's own at
    /// least.
    fn named(&mut self) -> io::Result<Vec<Seen>>;
    /// Opens `pid` to check and end it; `None` when no process has that pid any more.
    fn open(&mut self, pid: u32) -> io::Result<Option<Self::Process>>;
    /// The pids of every process in this process's own job, asked for with no job handle.
    fn job_pids(&mut self) -> io::Result<Vec<u32>>;
    /// When it was created, in 100 ns ticks (`FILETIME`).
    fn started(&mut self, process: &Self::Process) -> io::Result<u64>;
    /// When this process was created, in the same ticks: the earliest a process of the inner's
    /// job can have been created.
    fn own_start(&mut self) -> io::Result<u64>;
    /// Asks for it to end (`TerminateProcess`), without waiting: whether it ended is
    /// [`Processes::exited`]'s, never this call's success.
    fn kill(&mut self, process: &Self::Process) -> io::Result<()>;
    /// Whether it has exited (its handle signalled) within `wait`; `Duration::ZERO` doesn't
    /// wait.
    fn exited(&mut self, process: &Self::Process, wait: Duration) -> bool;
    /// Waits `pause` before the next look at a refused process.
    fn pause(&mut self, pause: Duration);
    /// A monotonic clock's reading, from any origin: the sweep's deadline is on it.
    fn now(&mut self) -> Duration;
}

/// Why a sweep failed. Its text names a pid, an image name and an OS code, nothing else.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SweepError {
    /// The process list couldn't be read.
    #[error("the process list failed ({0})")]
    List(String),
    /// One of the pool user's processes couldn't be opened.
    #[error("the sandbox user's process {pid} ({image}) can't be opened ({code})")]
    Open {
        pid: u32,
        image: String,
        code: String,
    },
    /// The pid list of this runner's own job couldn't be read.
    #[error("the job's process list failed ({0})")]
    JobList(String),
    /// One of them couldn't be ended.
    #[error("the sandbox user's process {pid} ({image}) can't be ended ({code})")]
    Kill {
        pid: u32,
        image: String,
        code: String,
    },
    /// One was asked to end and its handle still wasn't signalled [`KILL_WAIT`] later.
    #[error(
        "the sandbox user's process {pid} ({image}) still runs {secs} s after its kill",
        secs = KILL_WAIT.as_secs()
    )]
    Survived { pid: u32, image: String },
    /// The pool user still had new processes after every round.
    #[error("the sandbox user still started processes after {ROUNDS} rounds")]
    StillRunning,
    /// The sweep's waits reached its deadline with processes still to look at.
    #[error("the sweep ran out of its time with the sandbox user's processes still to end")]
    Deadline,
    /// The creation time of one of them couldn't be read.
    #[error("the sandbox user's process {pid} ({image}) has no start time ({code})")]
    Started {
        pid: u32,
        image: String,
        code: String,
    },
    /// At a point but the first: this many of them were left, each logged; `unkillable` of
    /// them couldn't be opened or ended (escapees), whose image names are `images` (each once,
    /// at most [`IMAGES_CAP`]).
    #[error("{count} of the sandbox user's processes were left (each logged)")]
    Left {
        count: usize,
        unkillable: usize,
        images: Vec<String>,
    },
    /// The sweeping runner's own creation time couldn't be read.
    #[error("the sweeping runner has no start time ({code})")]
    OwnStart { code: String },
}

/// The pool user's processes a sweep couldn't end: how many, and their image names (each once,
/// at most [`IMAGES_CAP`]; never a path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Escapees {
    pub(crate) count: usize,
    pub(crate) images: Vec<String>,
}

impl SweepError {
    /// The escapees this failure names: a process that couldn't be opened or ended, or that
    /// survived its kill, or the ones [`SweepError::Left`] counts so; `None` for any other
    /// failure.
    pub(crate) fn escapees(&self) -> Option<Escapees> {
        match self {
            SweepError::Open { image, .. }
            | SweepError::Kill { image, .. }
            | SweepError::Survived { image, .. } => Some(Escapees {
                count: 1,
                images: vec![image.clone()],
            }),
            SweepError::Left {
                unkillable, images, ..
            } if *unkillable > 0 => Some(Escapees {
                count: *unkillable,
                images: images.clone(),
            }),
            SweepError::List(_)
            | SweepError::JobList(_)
            | SweepError::StillRunning
            | SweepError::Deadline
            | SweepError::Started { .. }
            | SweepError::Left { .. }
            | SweepError::OwnStart { .. } => None,
        }
    }
}

/// Kills every process of `user` but those in `keep` (this process's own pid) and, past the
/// first act, the inner's job's ([`job_kept`]); each kill and, past the first act, each failure
/// is a line given to `log`. The pids it ended: killed, or exited by themselves once refused.
pub(crate) fn sweep<P: Processes>(
    calls: &mut P,
    user: &Sid,
    keep: &[u32],
    point: Point,
    log: &mut dyn FnMut(&str),
) -> Result<Vec<u32>, SweepError> {
    let mut killed = Killed::new();
    let deadline = calls.now().saturating_add(SWEEP_DEADLINE);
    let own = calls
        .own_start()
        .map_err(|e| SweepError::OwnStart { code: code(&e) })?;
    if point == Point::Start {
        // Nothing is kept as the job's here; the query is made so that one that fails stops
        // the launch now.
        calls
            .job_pids()
            .map_err(|e| SweepError::JobList(code(&e)))?;
    }
    let mut failures = Failures {
        point,
        logged: BTreeSet::new(),
        count: 0,
        unkillable: 0,
        images: Vec::new(),
        this_round: BTreeSet::new(),
    };
    for _ in 0..ROUNDS {
        failures.next_round();
        // Out of time: the held handles are the verdict, each survivor's looked at once more.
        if calls.now() >= deadline {
            kill::settle(calls, &mut killed, point, &mut failures, log)?;
            return Err(failures.verdict(SweepError::Deadline));
        }
        let seen = list(calls)?;
        let mut opened = Vec::new();
        for found in seen
            .iter()
            .filter(|s| is(s, user) && !keep.contains(&s.pid))
        {
            match calls.open(found.pid) {
                Ok(Some(process)) => opened.push((found, process)),
                Ok(None) => {}
                Err(error) => {
                    // No handle: counted every round from now on, however it is listed.
                    let error = failed(found, &error, Step::Open);
                    failures.add(error.clone(), log)?;
                    killed.unopened(error);
                }
            }
        }
        // Each pid is pinned now: one the lists still give `user` is the process opened.
        let again = if opened.is_empty() {
            Vec::new()
        } else {
            list(calls)?
        };
        // Read after the opens, so a pid in it is the process opened, and every round, so a
        // process the agent started since the last one is in it.
        let job = match point {
            Point::Tick | Point::End if !opened.is_empty() => calls
                .job_pids()
                .map_err(|e| SweepError::JobList(code(&e)))?,
            Point::Start | Point::Tick | Point::End => Vec::new(),
        };
        let rule = Rule {
            point,
            own,
            job,
            deadline,
        };
        let mut round = 0usize;
        let mut refused = Vec::new();
        for (found, process) in opened {
            if !again.iter().any(|s| s.pid == found.pid && is(s, user)) {
                continue;
            }
            match kill::one(calls, (found, process), &rule, &mut killed, log) {
                Done::Nothing => {}
                Done::Killed => round += 1,
                Done::Refused(one) => refused.push(one),
                Done::Failed(error) => failures.add(error, log)?,
            }
        }
        let budget = deadline.saturating_sub(calls.now());
        let (ended, left) = refused::wait_out(calls, refused, budget);
        for gone in ended {
            log(&gone.exited_line(point));
            killed.ended(gone.found.pid, gone.started, gone.process);
            round += 1;
        }
        for still in left {
            // Held: its handle is looked at every round from now on, listed or not.
            let error = failed(still.found, &still.error, Step::Kill);
            failures.add(error.clone(), log)?;
            killed.survivor(still.found, still.started, still.process, error);
        }
        round += kill::settle(calls, &mut killed, point, &mut failures, log)?;
        if round == 0 {
            return match failures.count {
                0 => Ok(killed.pids()),
                _ => Err(failures.left()),
            };
        }
    }
    Err(failures.verdict(SweepError::StillRunning))
}

/// What a round keeps besides `keep`.
struct Rule {
    point: Point,
    /// The sweeping runner's creation time.
    own: u64,
    /// The pids of its job's processes, read this round; empty where none is kept.
    job: Vec<u32>,
    /// When the sweep's waits end ([`Processes::now`]).
    deadline: Duration,
}

/// Whether the process `pid`, created at `started`, is kept as one of the inner's job: `job`,
/// the job's pid list, holds it, and it was created no earlier than the inner (`own`). One
/// created earlier can't be in the job: its pid is a reused one.
pub(crate) fn job_kept(job: &[u32], pid: u32, started: u64, own: u64) -> bool {
    job.contains(&pid) && started >= own
}

/// Whether the process `found`, created at `created` (as read), is kept: it is the job's
/// ([`job_kept`]). A creation time that couldn't be read fails only for a pid in the job's list,
/// where it decides.
fn spared(found: &Seen, created: io::Result<u64>, rule: &Rule) -> io::Result<bool> {
    if !rule.job.contains(&found.pid) {
        return Ok(false);
    }
    Ok(job_kept(&rule.job, found.pid, created?, rule.own))
}

/// Both lists as one: a pid either gives a user has that user, the opening list's first. A list
/// that can't be read fails the sweep.
fn list<P: Processes>(calls: &mut P) -> Result<Vec<Seen>, SweepError> {
    let mut seen = calls.list().map_err(|e| SweepError::List(code(&e)))?;
    let named = calls.named().map_err(|e| SweepError::List(code(&e)))?;
    for entry in named {
        if entry.owner == Owner::None {
            continue;
        }
        match seen.iter_mut().find(|s| s.pid == entry.pid) {
            Some(known) if known.owner == Owner::None => known.owner = entry.owner,
            Some(_) => {}
            None => seen.push(entry),
        }
    }
    Ok(seen)
}

fn is(seen: &Seen, user: &Sid) -> bool {
    matches!(&seen.owner, Owner::User(owner) if owner == user)
}

#[derive(Clone, Copy)]
enum Step {
    Open,
    Started,
    Kill,
}

fn failed(found: &Seen, error: &io::Error, step: Step) -> SweepError {
    let (pid, image, code) = (found.pid, found.image.clone(), code(error));
    match step {
        Step::Open => SweepError::Open { pid, image, code },
        Step::Started => SweepError::Started { pid, image, code },
        Step::Kill => SweepError::Kill { pid, image, code },
    }
}

/// An error's OS code, or its kind.
fn code(error: &io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => code.to_string(),
        None => error.kind().to_string(),
    }
}

#[cfg(test)]
mod tests;
