//! The sweep of the pool user's processes (M1): the outer runner kills every process whose token
//! user is the pool user, except its own inner runner and what is in its kill-on-close job.
//!
//! A pool user serves one live session at a time (`pool::assign::busy`), so no process of it
//! outside this launch's job belongs to a live session. The outer sweeps:
//!
//! - (a) before the pool user's logon ([`Point::BeforeLogon`]);
//! - (b) once the suspended inner runner is in the job and its process and thread are shut, and
//!   before it is resumed ([`Point::BeforeResume`]): a process that opened the inner's token in
//!   the moment between `CreateProcessWithLogonW` and the shut, one the logon set off among them,
//!   is killed before the inner runs any of its code;
//! - every [`TICK`] while the session runs ([`Point::Tick`], [`watch`]): a process some Windows
//!   service started outside the job lives at most that long;
//! - (c) at session end ([`Point::End`]).
//!
//! At (a) and (b) a failure fails the launch closed: a list that can't be read, or a process of
//! the pool user's that can't be opened, checked against the job or ended. A process the list
//! gives no user for ([`Owner::None`]) is not the pool user's and is passed over. At a tick and
//! at (c) a failure is logged and the session goes on; a tick's failure is tried again at the
//! next tick.
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

/// How often the outer sweeps while the session runs.
pub(crate) const TICK: Duration = Duration::from_secs(30);
/// How many rounds one sweep makes: a round that kills something is followed by another, for
/// what those processes started meanwhile.
const ROUNDS: usize = 8;

/// Where the sweep runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Point {
    /// (a): before the pool user's logon.
    BeforeLogon,
    /// (b): the inner runner in the job and shut, before it is resumed.
    BeforeResume,
    /// While the session runs, every [`TICK`].
    Tick,
    /// (c): at session end.
    End,
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Point::BeforeLogon => "before logon",
            Point::BeforeResume => "before resume",
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
    /// No user: `WTSEnumerateProcessesW` reads each token in the service, as SYSTEM, and gives
    /// none only for the kernel's own and pseudo processes (System Idle, System, Registry, Memory
    /// Compression, Secure System) and protected ones, none of which a pool user can start. Not
    /// the pool user's, so passed over.
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
    /// Every process on the machine, with its user.
    fn list(&mut self) -> io::Result<Vec<Seen>>;
    /// Opens `pid` to check and end it; `None` when no process has that pid any more.
    fn open(&mut self, pid: u32) -> io::Result<Option<Self::Process>>;
    /// Whether it is in this launch's kill-on-close job.
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

/// Kills every process of `user` but those in `keep` (pids this runner holds a handle to: its
/// inner runner) and those in the job, logging each kill at `point`. The pids it killed.
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

/// Sweeps at [`Point::Tick`] each time `wait(interval)` gives `None`, until it gives the
/// session's end, which it returns. A tick's failure is logged; the next tick tries again.
pub(crate) fn watch<P: Processes, T>(
    calls: &mut P,
    user: &Sid,
    keep: &[u32],
    interval: Duration,
    mut wait: impl FnMut(Duration) -> Option<T>,
    log: &mut dyn FnMut(&str),
) -> T {
    loop {
        if let Some(ended) = wait(interval) {
            return ended;
        }
        if let Err(error) = sweep(calls, user, keep, Point::Tick, log) {
            log(&format!(
                "clauseo-sandbox-run: sweep ({}): {error}",
                Point::Tick
            ));
        }
    }
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
