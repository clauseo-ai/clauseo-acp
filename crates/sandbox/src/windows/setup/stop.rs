//! `clauseo-sandbox-setup.exe --stop` (0049, "Stopping the pool's processes"), run by the SYSTEM
//! task `\Clauseo\SandboxStop`: stops every process whose token user is a pool user, whatever
//! its DACL says. The runner's sweeps stop a matter's processes as the pool user; a process that
//! shut its own DACL survives them and could stay alive into the next logon. SYSTEM, with
//! `SeDebugPrivilege`, opens it regardless.
//!
//! [`stop`] holds the order and the refusals, with no Windows call; every effect goes through
//! [`Stopper`], which `windows_stopper.rs` implements and the tests fake. It:
//!
//! 1. refuses unless this process is LOCAL SYSTEM and runs from the admin copy's path;
//! 2. reads the pool from the machine's record, `HKLM\SOFTWARE\Clauseo\Sandbox`: `PoolSids` and
//!    `OwnerSid`, which only the elevated set-up writes. Nothing else names a SID: no argument
//!    (the task passes `--stop` alone, `args.rs`), no environment, nothing under the lawyer's
//!    data folder. No record, or an empty pool, is nothing to stop;
//! 3. holds every SID to the pool-user check (`strays::check_pool_sid`, as `processes.rs` does
//!    before a profile delete): a local account of this machine, not built in, not SYSTEM, not
//!    the owner, named `ClauseoSbNN` when it resolves. One that fails stops nothing;
//! 4. takes a process snapshot, picks each process whose token user is exactly one of the pool's
//!    SIDs (`strays::to_stop`, never this process, pid 0 or pid 4, never a process whose user
//!    couldn't be read) and terminates each through the handle its user was read through, so a
//!    reused pid can't redirect it; then does it all again, for what started meanwhile or didn't
//!    end;
//! 5. adds each pool user it found running to the record's `ResetSids`, under the set-up lock (a
//!    bounded wait), so the next set-up deletes and remakes its profile before the user runs
//!    anything again (`resets.rs`): what the stopped process left in its profile goes with it.
//!
//! The reset itself isn't done here: it sets a new password, which only the owner's
//! `secrets.bin` may hold (DPAPI, as the owner), and its open-session check and its `LocalLow`
//! denies read and write the owner's data folder, as the owner. SYSTEM has no owner token here,
//! so the reset waits for the next set-up.
//!
//! It logs counts only, one line, and exits with its own code ([`StopExit`]).

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::strays::{Seen, to_stop};
use crate::windows::refresh::same_path;
use crate::windows::sids::Sid;

#[cfg(test)]
mod tests;
#[cfg(windows)]
pub(crate) mod windows_stopper;

/// The stop's exit codes. They start at 45, after [`RefreshExit`]'s 44, so the one exe never
/// gives one number two meanings; codes are appended from here on, never renumbered.
///
/// [`RefreshExit`]: crate::windows::refresh::RefreshExit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopExit {
    /// 0: every pool user's process found was stopped (or none ran), and those users are marked.
    Done,
    /// 45: not LOCAL SYSTEM, or not run from the admin copy's path; nothing was stopped.
    NotAllowed,
    /// 46: the record can't be read, or its `PoolSids` or `OwnerSid` is malformed; nothing was
    /// stopped.
    NoRecord,
    /// 47: a SID `PoolSids` lists isn't a pool user's; nothing was stopped.
    NotPool,
    /// 48: a pool user's process was still there after the second pass, or a process snapshot
    /// couldn't be taken.
    NotStopped,
    /// 49: the processes were stopped, but their users couldn't be marked in `ResetSids` (the
    /// set-up lock stayed held, or the write failed).
    NotMarked,
}

impl StopExit {
    /// The process exit code.
    pub fn as_code(self) -> u8 {
        match self {
            StopExit::Done => 0,
            StopExit::NotAllowed => 45,
            StopExit::NoRecord => 46,
            StopExit::NotPool => 47,
            StopExit::NotStopped => 48,
            StopExit::NotMarked => 49,
        }
    }
}

/// What the record gives the stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pool {
    /// `OwnerSid`: never stopped, whatever `PoolSids` says.
    pub owner: Sid,
    /// `PoolSids`.
    pub users: Vec<Sid>,
}

/// A failed effect: what failed, as a line for the log.
pub type Failed = String;

/// Every effect of the stop. The real one reads the record and the admin copy's path itself, so
/// no method takes a SID or a path the caller chose, but the pool the record gave.
pub trait Stopper {
    /// Whether this process's token user is LOCAL SYSTEM.
    fn is_system(&mut self) -> Result<bool, Failed>;
    /// This process's exe, its final path.
    fn current_exe(&mut self) -> Result<PathBuf, Failed>;
    /// The admin copy's path: `<FOLDERID_ProgramFiles>\Clauseo\Sandbox\clauseo-sandbox-setup.exe`.
    fn admin_copy(&mut self) -> Result<PathBuf, Failed>;
    /// The machine record's `OwnerSid` and `PoolSids`; `None` when there is no record.
    fn pool(&mut self) -> Result<Option<Pool>, Failed>;
    /// Whether every SID of `pool.users` passes the pool-user check (`strays::check_pool_sid`).
    fn check_pool(&mut self, pool: &Pool) -> Result<(), Failed>;
    /// Every running process with its token user, from a fresh snapshot; the handles it opened
    /// replace the last snapshot's, for [`Stopper::terminate`].
    fn snapshot(&mut self) -> Result<Vec<Seen>, Failed>;
    /// This process's id.
    fn own_pid(&mut self) -> u32;
    /// Terminates `pid` through the handle the last snapshot read its user through.
    fn terminate(&mut self, pid: u32) -> Result<(), Failed>;
    /// Waits a bounded time for the set-up lock and keeps it until [`Stopper::unlock`]:
    /// `Ok(false)` when another still holds it.
    fn lock(&mut self) -> Result<bool, Failed>;
    /// Gives the set-up lock back, if held.
    fn unlock(&mut self);
    /// Adds `users` to the record's `ResetSids`, keeping what it lists.
    fn mark_for_reset(&mut self, users: &[Sid]) -> Result<(), Failed>;
    /// Writes one line to the log: `error` for a refusal or a failure.
    fn log(&mut self, error: bool, line: &str);
}

/// What the passes did.
#[derive(Debug, Default)]
struct Passes {
    /// The pool users seen running, in either pass.
    found: BTreeSet<Sid>,
    /// Processes terminated, both passes.
    stopped: usize,
    /// Terminations that failed in the first pass.
    first_failed: usize,
    /// Terminations that failed in the second pass: processes still there.
    left: usize,
}

/// Runs the stop as the module says.
pub fn stop(fs: &mut impl Stopper) -> StopExit {
    match run_steps(fs) {
        Ok(line) => {
            fs.log(false, &line);
            StopExit::Done
        }
        Err((exit, line)) => {
            fs.log(true, &format!("the sandbox's stop: {line}"));
            exit
        }
    }
}

/// A refusal or failure: its exit and its log line.
type Refusal = (StopExit, String);

/// The steps; `Ok` carries the log line of a run that stopped and marked everything it found.
fn run_steps(fs: &mut impl Stopper) -> Result<String, Refusal> {
    allowed(fs)?;
    let pool = match fs.pool().map_err(|why| (StopExit::NoRecord, why))? {
        Some(pool) if !pool.users.is_empty() => pool,
        _ => return Ok("the sandbox's stop: no pool users are recorded".to_owned()),
    };
    fs.check_pool(&pool)
        .map_err(|why| (StopExit::NotPool, format!("nothing was stopped: {why}")))?;
    let mut passes = Passes::default();
    let first = pass(fs, &pool.users, &mut passes, false);
    let second = pass(fs, &pool.users, &mut passes, true);
    let counts = format!(
        "{} pool users, {} found running, {} processes stopped, {} left",
        pool.users.len(),
        passes.found.len(),
        passes.stopped,
        passes.left
    );
    let marked = mark(fs, &passes.found);
    if let Err(why) = first.and(second) {
        return Err((StopExit::NotStopped, format!("{counts}; {why}")));
    }
    if passes.left > 0 {
        let why = "a pool user's process was still there after the second pass";
        return Err((StopExit::NotStopped, format!("{counts}; {why}")));
    }
    marked.map_err(|why| (StopExit::NotMarked, format!("{counts}; {why}")))?;
    let tried = passes.first_failed;
    Ok(format!(
        "the sandbox's stop: {counts}; {} users marked for a profile reset ({tried} retried)",
        passes.found.len()
    ))
}

/// Step 1: LOCAL SYSTEM, from the admin copy's path.
fn allowed(fs: &mut impl Stopper) -> Result<(), Refusal> {
    let refuse = |why: String| (StopExit::NotAllowed, why);
    if !fs.is_system().map_err(refuse)? {
        return Err(refuse("it runs only as LOCAL SYSTEM".to_owned()));
    }
    let exe = fs.current_exe().map_err(refuse)?;
    let admin = fs.admin_copy().map_err(refuse)?;
    if !same_path(&exe, &admin) {
        return Err(refuse("it runs only from the admin copy".to_owned()));
    }
    Ok(())
}

/// Step 4, once: a snapshot, then each pool user's process terminated, counted in `passes`.
/// A snapshot that can't be taken is the error; a failed termination is counted, and the
/// `second` pass's are the processes left.
fn pass(
    fs: &mut impl Stopper,
    pool: &[Sid],
    passes: &mut Passes,
    second: bool,
) -> Result<(), String> {
    let seen = fs
        .snapshot()
        .map_err(|why| format!("a process snapshot failed: {why}"))?;
    let own = fs.own_pid();
    let mut failed = 0;
    for pid in to_stop(&seen, pool, own) {
        let user = seen.iter().find(|process| process.pid == pid);
        if let Some(user) = user.and_then(|process| process.user.clone()) {
            passes.found.insert(user);
        }
        match fs.terminate(pid) {
            Ok(()) => passes.stopped += 1,
            Err(_why) => failed += 1,
        }
    }
    if second {
        passes.left = failed;
    } else {
        passes.first_failed = failed;
    }
    Ok(())
}

/// Step 5: `found` added to `ResetSids` under the set-up lock; nothing to do when it is empty.
fn mark(fs: &mut impl Stopper, found: &BTreeSet<Sid>) -> Result<(), String> {
    if found.is_empty() {
        return Ok(());
    }
    if !fs.lock()? {
        return Err("the set-up lock stayed held; no user was marked".to_owned());
    }
    let users: Vec<Sid> = found.iter().cloned().collect();
    let marked = fs.mark_for_reset(&users);
    fs.unlock();
    marked
}
