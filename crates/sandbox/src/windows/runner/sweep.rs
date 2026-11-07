//! The sweep of the pool user's processes (M1): every process whose token user is the pool user
//! is killed, but the sweeping process itself and, mid-session, what is in this launch's
//! kill-on-close job. It runs as the pool user, which can list its own processes; the desktop
//! user can't (`WTSEnumerateProcessesW` lists another user's processes only to Administrators,
//! and the outer runs unelevated), so the outer sweeps nothing.
//!
//! A pool user serves one live session at a time (`pool::assign::busy`), so no process of it
//! outside this launch's job belongs to a live session, but for a next session's, which only the
//! end sweeper can meet (below). The sweeps:
//!
//! - [`Point::Start`]: the inner runner's first act once resumed (`inner_flow.rs`), before it
//!   reads the launch or makes anything. Only the inner itself is kept: nothing else can be in
//!   the job yet (the job is unnamed, its only full handle is the outer's, and the inner has
//!   started nothing). The first failure fails the launch closed (`RunExit::Sweep`).
//! - [`Point::Tick`]: every [`TICK`] while the agent runs, in the inner, keeping the job's
//!   processes: one some Windows service started outside the job lives at most that long.
//! - [`Point::End`]: in the inner, once the agent exited, keeping the job's processes (the outer
//!   ends the job itself).
//! - [`Point::Sweeper`]: the detached end sweeper (`--sweep`, `sweeper.rs`), which keeps
//!   nothing but itself, and kills only processes created before it was
//!   ([`Processes::own_start`]).
//!
//! At every point but the first, a process that can't be opened, checked or ended is logged and
//! the sweep goes on with the rest; it is tried again in the sweep's next round and at the next
//! tick, and the sweep ends in [`SweepError::Left`].
//!
//! **Two listings.** [`Processes::list`] reads each process's user by opening it, which fails
//! for a process whose DACL shuts the pool user out (one that set its own, with an `OWNER
//! RIGHTS` entry so its owner keeps no implied `WRITE_DAC`). [`Processes::named`] gives the
//! user of each of the caller's own processes whatever their DACL (`WTSEnumerateProcessesW`,
//! whose service reads the tokens). A pid either names the pool user is the pool user's, so such
//! a process is found, fails its open, and fails the first act closed.
//!
//! **The end sweeper and a next session.** The end sweeper lives up to 60 s, and a next launch
//! for the same pool user may start meanwhile. It kills only what was created before it started:
//! the next session's processes start later and are left alone, and a process an escaped one
//! starts after the sweeper started is killed by the next session's first act or tick. The other
//! way round, the next inner's first act kills a sweeper still running (a pool-user process
//! outside its job), which loses nothing: that inner makes the same cleanup itself
//! (`leftovers.rs`). Each can open the other: the runners' process DACL grants the desktop
//! user's logon SID, which both hold (`token::runner_process_sddl`).
//!
//! Each process found is opened first, which pins its pid, and then the lists are read again: only
//! a pid they still give the pool user is killed, so a pid reused meanwhile is left alone.
//! The calls are behind [`Processes`]; Windows' is `os.rs`.

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::time::Duration;

use crate::windows::sids::Sid;

#[cfg(windows)]
pub(super) mod os;

/// How often the inner sweeps while the agent runs.
pub(crate) const TICK: Duration = Duration::from_secs(30);
/// How many rounds one sweep makes: a round that kills something is followed by another, for
/// what those processes started meanwhile.
const ROUNDS: usize = 8;
/// The exit code a swept process ends with: the runner's `RunExit` range's last code.
pub(crate) const SWEPT: u32 = super::RUN_EXIT_BASE + 0xff;

/// Where the sweep runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Point {
    /// The inner runner's first act, before it reads the launch.
    Start,
    /// While the agent runs, every [`TICK`].
    Tick,
    /// In the inner, once the agent exited.
    End,
    /// The detached end sweeper.
    Sweeper,
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Point::Start => "before launch",
            Point::Tick => "mid-session",
            Point::End => "session end",
            Point::Sweeper => "end sweeper",
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
    /// Whether it is in this launch's kill-on-close job; always false for a sweep that keeps
    /// no job's processes.
    fn in_job(&mut self, process: &Self::Process) -> io::Result<bool>;
    /// When it was created, in 100 ns ticks (`FILETIME`).
    fn started(&mut self, process: &Self::Process) -> io::Result<u64>;
    /// When this process was created, in the same ticks: the end sweeper's cutoff.
    fn own_start(&mut self) -> io::Result<u64>;
    /// Ends it, and waits until it has ended.
    fn kill(&mut self, process: &Self::Process) -> io::Result<()>;
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
    /// One of them couldn't be checked against the job.
    #[error("the sandbox user's process {pid} ({image}) can't be checked ({code})")]
    Job {
        pid: u32,
        image: String,
        code: String,
    },
    /// One of them couldn't be ended.
    #[error("the sandbox user's process {pid} ({image}) can't be ended ({code})")]
    Kill {
        pid: u32,
        image: String,
        code: String,
    },
    /// The pool user still had new processes after every round.
    #[error("the sandbox user still started processes after {ROUNDS} rounds")]
    StillRunning,
    /// The creation time of one of them, or of the end sweeper itself, couldn't be read.
    #[error("the sandbox user's process {pid} ({image}) has no start time ({code})")]
    Started {
        pid: u32,
        image: String,
        code: String,
    },
    /// At a point but the first: this many of them were left, each logged.
    #[error("{count} of the sandbox user's processes were left (each logged)")]
    Left { count: usize },
}

/// Kills every process of `user` but those in `keep` (this process's own pid) and those in the
/// job, and, for the end sweeper, those created since it was; each kill and, past the first act,
/// each failure is a line given to `log`. The pids it killed.
pub(crate) fn sweep<P: Processes>(
    calls: &mut P,
    user: &Sid,
    keep: &[u32],
    point: Point,
    log: &mut dyn FnMut(&str),
) -> Result<Vec<u32>, SweepError> {
    let before = match point {
        Point::Sweeper => Some(calls.own_start().map_err(|e| SweepError::Started {
            pid: keep.first().copied().unwrap_or_default(),
            image: String::from("the end sweeper"),
            code: code(&e),
        })?),
        Point::Start | Point::Tick | Point::End => None,
    };
    let mut killed = Vec::new();
    let mut failures = Failures {
        point,
        logged: BTreeSet::new(),
        count: 0,
    };
    for _ in 0..ROUNDS {
        failures.count = 0;
        let seen = list(calls)?;
        let mut opened = Vec::new();
        for found in seen
            .iter()
            .filter(|s| is(s, user) && !keep.contains(&s.pid))
        {
            match calls.open(found.pid) {
                Ok(Some(process)) => opened.push((found, process)),
                Ok(None) => {}
                Err(error) => failures.add(failed(found, &error, Step::Open), log)?,
            }
        }
        // Each pid is pinned now: one the lists still give `user` is the process opened.
        let again = if opened.is_empty() {
            Vec::new()
        } else {
            list(calls)?
        };
        let mut round = 0usize;
        for (found, process) in &opened {
            if !again.iter().any(|s| s.pid == found.pid && is(s, user)) {
                continue;
            }
            match spared(calls, process, before) {
                Ok(true) => continue,
                Ok(false) => {}
                Err((error, step)) => {
                    failures.add(failed(found, &error, step), log)?;
                    continue;
                }
            }
            if let Err(error) = calls.kill(process) {
                failures.add(failed(found, &error, Step::Kill), log)?;
                continue;
            }
            log(&format!(
                "clauseo-sandbox-run: sweep ({point}) killed the sandbox user's process {} ({})",
                found.pid, found.image
            ));
            killed.push(found.pid);
            round += 1;
        }
        if round == 0 {
            return match failures.count {
                0 => Ok(killed),
                count => Err(SweepError::Left { count }),
            };
        }
    }
    Err(SweepError::StillRunning)
}

/// Whether `process` is kept: it is in the job, or it started at or after `before`.
fn spared<P: Processes>(
    calls: &mut P,
    process: &P::Process,
    before: Option<u64>,
) -> Result<bool, (io::Error, Step)> {
    if calls.in_job(process).map_err(|e| (e, Step::Job))? {
        return Ok(true);
    }
    let Some(before) = before else {
        return Ok(false);
    };
    let started = calls.started(process).map_err(|e| (e, Step::Started))?;
    Ok(started >= before)
}

/// A round's failures: the first act's first ends the sweep; past it, each is logged once a
/// sweep and counted for the round.
struct Failures {
    point: Point,
    logged: BTreeSet<u32>,
    count: usize,
}

impl Failures {
    fn add(&mut self, error: SweepError, log: &mut dyn FnMut(&str)) -> Result<(), SweepError> {
        if self.point == Point::Start {
            return Err(error);
        }
        self.count += 1;
        if self.logged.insert(pid_of(&error)) {
            log(&format!(
                "clauseo-sandbox-run: sweep ({}): {error}",
                self.point
            ));
        }
        Ok(())
    }
}

fn pid_of(error: &SweepError) -> u32 {
    match error {
        SweepError::Open { pid, .. }
        | SweepError::Job { pid, .. }
        | SweepError::Kill { pid, .. }
        | SweepError::Started { pid, .. } => *pid,
        SweepError::List(_) | SweepError::StillRunning | SweepError::Left { .. } => 0,
    }
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
    Job,
    Started,
    Kill,
}

fn failed(found: &Seen, error: &io::Error, step: Step) -> SweepError {
    let (pid, image, code) = (found.pid, found.image.clone(), code(error));
    match step {
        Step::Open => SweepError::Open { pid, image, code },
        Step::Job => SweepError::Job { pid, image, code },
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
