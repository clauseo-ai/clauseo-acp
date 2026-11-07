//! The sweep of the pool user's processes (M1): every process whose token user is the pool user
//! is killed, but the sweeping process itself and, mid-session, what is in this launch's
//! kill-on-close job. It runs as the pool user, which can always list and open its own
//! processes; the desktop user can't (`WTSEnumerateProcessesW` lists another user's processes
//! only to Administrators, and the outer runs unelevated), so the outer sweeps nothing.
//!
//! A pool user serves one live session at a time (`pool::assign::busy`), so no process of it
//! outside this launch's job belongs to a live session. The sweeps:
//!
//! - [`Point::Start`]: the inner runner's first act once resumed (`inner_flow.rs`), before it
//!   reads the launch or makes anything. Only the inner itself is kept: nothing else can be in
//!   the job yet (the job is unnamed, its only full handle is the outer's, and the inner has
//!   started nothing). A failure fails the launch closed (`RunExit::Sweep`).
//! - [`Point::Tick`]: every [`TICK`] while the agent runs, in the inner, keeping the job's
//!   processes: one some Windows service started outside the job lives at most that long. A
//!   failure is logged and tried again at the next tick.
//! - [`Point::End`]: in the inner, once the agent exited, keeping the job's processes (the outer
//!   ends the job itself). A failure is logged.
//! - [`Point::Sweeper`]: the detached end sweeper (`--sweep`, `sweeper.rs`), which keeps
//!   nothing but itself. A failure is its exit code.
//!
//! A process whose user this process can't read ([`Owner::None`]) is passed over.
//!
//! Each process found is opened first, which pins its pid, and then the list is read again: only
//! a pid that list still gives the pool user is killed, so a pid reused meanwhile is left alone.
//! The calls are behind [`Processes`]; Windows' is `os.rs`.

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
    /// `PROCESS_QUERY_LIMITED_INFORMATION`, or its token with `TOKEN_QUERY`. Another user's
    /// process, a protected one, or the kernel's own; the pool user's own processes open for it
    /// (their default DACL grants their user full access). Passed over.
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
    /// Every process on the machine, with its user as far as this process can read it.
    fn list(&mut self) -> io::Result<Vec<Seen>>;
    /// Opens `pid` to check and end it; `None` when no process has that pid any more.
    fn open(&mut self, pid: u32) -> io::Result<Option<Self::Process>>;
    /// Whether it is in this launch's kill-on-close job; always false for a sweep that keeps
    /// no job's processes.
    fn in_job(&mut self, process: &Self::Process) -> io::Result<bool>;
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
}

/// Kills every process of `user` but those in `keep` (this process's own pid) and those in the
/// job, logging each kill at `point`. The pids it killed.
pub(crate) fn sweep<P: Processes>(
    calls: &mut P,
    user: &Sid,
    keep: &[u32],
    point: Point,
    log: &mut dyn FnMut(&str),
) -> Result<Vec<u32>, SweepError> {
    let mut killed = Vec::new();
    for _ in 0..ROUNDS {
        let seen = list(calls)?;
        let mut opened = Vec::new();
        for found in seen
            .iter()
            .filter(|s| is(s, user) && !keep.contains(&s.pid))
        {
            match calls.open(found.pid) {
                Ok(Some(process)) => opened.push((found, process)),
                Ok(None) => {}
                Err(error) => return Err(failed(found, &error, Step::Open)),
            }
        }
        if opened.is_empty() {
            return Ok(killed);
        }
        // Each pid is pinned now: one the list still gives `user` is the process opened.
        let again = list(calls)?;
        let mut round = 0usize;
        for (found, process) in &opened {
            if !again.iter().any(|s| s.pid == found.pid && is(s, user)) {
                continue;
            }
            if calls
                .in_job(process)
                .map_err(|e| failed(found, &e, Step::Job))?
            {
                continue;
            }
            calls
                .kill(process)
                .map_err(|e| failed(found, &e, Step::Kill))?;
            log(&format!(
                "clauseo-sandbox-run: sweep ({point}) killed the sandbox user's process {} ({})",
                found.pid, found.image
            ));
            killed.push(found.pid);
            round += 1;
        }
        if round == 0 {
            return Ok(killed);
        }
    }
    Err(SweepError::StillRunning)
}

/// The list; one that can't be read fails the sweep.
fn list<P: Processes>(calls: &mut P) -> Result<Vec<Seen>, SweepError> {
    calls.list().map_err(|e| SweepError::List(code(&e)))
}

fn is(seen: &Seen, user: &Sid) -> bool {
    matches!(&seen.owner, Owner::User(owner) if owner == user)
}

#[derive(Clone, Copy)]
enum Step {
    Open,
    Job,
    Kill,
}

fn failed(found: &Seen, error: &io::Error, step: Step) -> SweepError {
    let (pid, image, code) = (found.pid, found.image.clone(), code(error));
    match step {
        Step::Open => SweepError::Open { pid, image, code },
        Step::Job => SweepError::Job { pid, image, code },
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
