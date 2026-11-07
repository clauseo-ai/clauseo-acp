//! `clauseo-sandbox-setup.exe --stop` (0049, "Stopping the pool's processes"), run by the SYSTEM
//! task `\Clauseo\SandboxStop`: stops every process whose user is a pool user, whatever its DACL
//! says. The runner's sweeps stop a matter's processes as the pool user; a process that
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
//! 4. passes over the processes until one finds none of the pool's: each takes a snapshot and
//!    reads each process's user ([`Process`]) from the kernel's process list
//!    (`WTSEnumerateProcessesExW`), which names it whatever the token's DACL says, with the
//!    token's user as a cross-check. It picks each process whose user is exactly one of the
//!    pool's SIDs ([`pick`]; never this process, pid 0 or pid 4), and marks each pool user it
//!    finds for a reset at once (step 6), before anything is stopped. It then suspends them all,
//!    and terminates each through the handle its user was read through, so a reused pid can't
//!    redirect it and a parent can't start another between two terminations. The passes stop
//!    once one finds no pool user's process, or after [`PASS_LIMIT`]; what is still there then
//!    is left (48). A process whose user neither the list nor the token gives is unknown (48) and
//!    never stopped, with two exceptions. Its owner is a pool SID: it is stopped (the owner may
//!    add a process, never clear one: an owner is its creator, not its user, and an elevated
//!    administrator's processes are owned by Administrators). Or it can't be opened to be
//!    terminated while the list was read (a protected process: one a pool user started would
//!    still be named by the list). A list that couldn't be read at all leaves no such exemption,
//!    so a run where it failed ends at 48 unless each process's token, or a pool owner, named it;
//! 5. cancels every BITS job a pool user owns (`BG_JOB_ENUM_ALL_USERS`, each job's owner), even
//!    when no process was found: a pool user's job doesn't run, but stays queued until it is
//!    cancelled. One that can't be cancelled, or whose owner can't be read, is 48;
//! 6. adds each pool user it found running, or owning a job, to the record's `ResetSids`, under the
//!    set-up lock (a bounded wait), so the next set-up deletes and remakes its profile before the
//!    user runs anything again (`resets.rs`): what the stopped process left in its profile goes
//!    with it. Each user is added as soon as it is found, so a run the task's 2-minute limit
//!    kills has marked every user it saw. A pass that finds a process of unknown user marks every
//!    pool user at once, before anything is stopped, as that process may be any of theirs. Once
//!    the lock stays held, or a write fails, no more are tried (49), so the lock's wait comes once
//!    at most.
//!
//! The reset itself isn't done here: it sets a new password, which only the owner's
//! `secrets.bin` may hold (DPAPI, as the owner), and its open-session check and its `LocalLow`
//! denies read and write the owner's data folder, as the owner. SYSTEM has no owner token here,
//! so the reset waits for the next set-up.
//!
//! It logs counts only, one line, and exits with its own code ([`StopExit`]).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use crate::windows::refresh::same_path;
use crate::windows::sids::Sid;

#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows_bits;
#[cfg(windows)]
pub(crate) mod windows_stopper;
#[cfg(windows)]
#[cfg(test)]
mod windows_tests;

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
    /// 48: a pool user's process was still there when the passes ran out of time, a process's
    /// user couldn't be read, a pool user's BITS job wasn't cancelled (or a job's owner couldn't
    /// be read), or a process snapshot or the BITS list couldn't be taken.
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

/// How long the passes go on while each still finds a pool user's process: well inside the
/// task's 2-minute limit, with [`LAST_WAITS`] and the lock's 30 s wait after it.
pub const PASS_LIMIT: Duration = Duration::from_secs(60);
/// How long a terminated process is waited for, at most: one still there is terminated again by
/// the next pass.
pub const EXIT_WAIT: Duration = Duration::from_secs(1);
/// How long the waits for terminated processes may run past [`PASS_LIMIT`], all together: a
/// pass that began just before it, with many processes that won't exit, then waits for none,
/// so the run ends inside the task's limit.
pub const LAST_WAITS: Duration = Duration::from_secs(15);

/// The idle process's pid.
const IDLE_PID: u32 = 0;
/// The System process's pid.
const SYSTEM_PID: u32 = 4;

/// One process as a snapshot saw it: its user as each source gave it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Process {
    /// Its process id.
    pub pid: u32,
    /// Its user as the kernel's process list (`WTSEnumerateProcessesExW`) gave it, past the
    /// token's DACL; `None` when the list named no SID for it, or didn't list it.
    pub listed: Option<Sid>,
    /// Its token's user, read through the handle; `None` when the token couldn't be read.
    pub token: Option<Sid>,
    /// Its process object's owner, read when the token couldn't be; `None` when not read.
    /// A standard user can't make another account the owner of a process, but the owner is the
    /// process's creator, which needn't be its user.
    pub owner: Option<Sid>,
    /// It was opened with `PROCESS_TERMINATE`.
    pub can_stop: bool,
    /// The kernel's process list couldn't be read for this snapshot, so `listed` is `None`
    /// for want of a list, not because the list named no one.
    pub list_failed: bool,
}

/// What [`pick`] made of a process's user.
#[derive(Debug, Clone, PartialEq, Eq)]
enum User {
    /// Read: the list's SID, or the token's when the list gave none, or the owner's when
    /// neither gave one and the owner is one of the pool's SIDs.
    Known(Sid),
    /// Neither the list nor the token gave one, and the owner isn't a pool SID (or wasn't
    /// read); or the list and the token disagree.
    Unknown,
    /// Not looked at: this process, pid 0 or 4, or one with no user from any source that can't
    /// be opened to be terminated, even with `SeDebugPrivilege`, while the list was read (a
    /// protected process; one a pool user started would still be named by the list).
    Exempt,
}

/// `process`'s user, as [`User`] says. The owner may add a process to the pool's, never clear
/// one: it is the process's creator, and an elevated administrator's processes, whoever runs
/// them, are owned by `Administrators`.
fn user_of(process: &Process, pool: &[Sid], own_pid: u32) -> User {
    if [own_pid, IDLE_PID, SYSTEM_PID].contains(&process.pid) {
        return User::Exempt;
    }
    match (&process.listed, &process.token, &process.owner) {
        (Some(listed), Some(token), _) if listed != token => User::Unknown,
        (Some(user), _, _) | (None, Some(user), _) => User::Known(user.clone()),
        (None, None, Some(owner)) if pool.contains(owner) => User::Known(owner.clone()),
        (None, None, Some(_)) => User::Unknown,
        (None, None, None) if process.can_stop || process.list_failed => User::Unknown,
        (None, None, None) => User::Exempt,
    }
}

/// What one snapshot holds for the stop.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Picked {
    /// Each process whose user is one of the pool's SIDs, with that user, in the order seen,
    /// each pid once.
    pub stop: Vec<(u32, Sid)>,
    /// The processes whose user couldn't be read ([`User::Unknown`]).
    pub unknown: usize,
}

/// The processes of `seen` to stop, and those whose user is unknown, as [`user_of`] reads
/// each; SIDs are compared exactly. `own_pid` is this process.
pub fn pick(seen: &[Process], pool: &[Sid], own_pid: u32) -> Picked {
    let mut picked = Picked::default();
    let mut looked: Vec<u32> = Vec::new();
    for process in seen {
        if looked.contains(&process.pid) {
            continue;
        }
        looked.push(process.pid);
        match user_of(process, pool, own_pid) {
            User::Known(user) if pool.contains(&user) => picked.stop.push((process.pid, user)),
            User::Unknown => picked.unknown += 1,
            User::Known(_) | User::Exempt => {}
        }
    }
    picked
}

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
    /// Every running process with its user as each source gave it, from a fresh snapshot; the
    /// handles it opened replace the last snapshot's, for [`Stopper::suspend`] and
    /// [`Stopper::terminate`].
    fn snapshot(&mut self) -> Result<Vec<Process>, Failed>;
    /// This process's id.
    fn own_pid(&mut self) -> u32;
    /// Suspends `pid` through the handle the last snapshot read its user through. Nothing
    /// resumes it: it is terminated next.
    fn suspend(&mut self, pid: u32) -> Result<(), Failed>;
    /// Terminates `pid` through the handle the last snapshot read its user through, and waits
    /// up to `wait` for it to exit: `Err` when it hadn't.
    fn terminate(&mut self, pid: u32, wait: Duration) -> Result<(), Failed>;
    /// The time since the stop began, from a monotonic clock.
    fn elapsed(&mut self) -> Duration;
    /// Waits a moment before the next pass, after a termination failed, so a process that
    /// refuses isn't asked again at once.
    fn pause(&mut self);
    /// Every BITS job on this machine, all users', each as its owner: `None` when it couldn't
    /// be read. The list replaces the last one, for [`Stopper::cancel_job`].
    fn bits_jobs(&mut self) -> Result<Vec<Option<Sid>>, Failed>;
    /// Cancels the job at `at` in the last list, which removes it.
    fn cancel_job(&mut self, at: usize) -> Result<(), Failed>;
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
    /// The pool users seen running, in any pass: stopped or not.
    found: BTreeSet<Sid>,
    /// Snapshots taken.
    taken: usize,
    /// Processes terminated.
    stopped: usize,
    /// Terminations that failed, each retried in the next pass.
    retried: usize,
    /// Pool users' processes the last snapshot found once [`PASS_LIMIT`] had passed.
    left: usize,
    /// Processes whose user the last snapshot couldn't read.
    unknown: usize,
    /// A snapshot's process list (`WTSEnumerateProcessesExW`) couldn't be read.
    list_failed: bool,
    /// Every pool user, once a pass found a process of unknown user, which may be any of
    /// theirs: marked as `found` is, but not counted as found running.
    suspects: BTreeSet<Sid>,
    /// The users of `found` and `suspects` added to `ResetSids`.
    marked: BTreeSet<Sid>,
    /// Why a mark failed; once set, no more are tried.
    mark_failed: Option<String>,
    /// The pool users' BITS jobs cancelled.
    jobs_cancelled: usize,
    /// BITS jobs not cancelled: a pool user's whose cancel failed, or one whose owner couldn't
    /// be read.
    jobs_failed: usize,
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
    let ran = run_passes(fs, &pool.users, &mut passes);
    let cancelled = cancel_jobs(fs, &pool.users, &mut passes);
    // Without the list, a process no token names is unknown: the line says why.
    let list = if passes.list_failed {
        " (the process list couldn't be read)"
    } else {
        ""
    };
    let counts = format!(
        "{} pool users, {} found running, {} processes stopped in {} passes, {} left, \
         {} unknown{list}, {} BITS jobs cancelled, {} not",
        pool.users.len(),
        passes.found.len(),
        passes.stopped,
        passes.taken,
        passes.left,
        passes.unknown,
        passes.jobs_cancelled,
        passes.jobs_failed
    );
    mark_new(fs, &mut passes);
    let not_stopped = |why: &str| Err((StopExit::NotStopped, format!("{counts}; {why}")));
    if let Err(why) = ran.and(cancelled) {
        return not_stopped(&why);
    }
    if passes.jobs_failed > 0 {
        return not_stopped("a BITS job wasn't cancelled, or its owner couldn't be read");
    }
    if passes.left > 0 {
        return not_stopped("a pool user's process was still there when the passes' time ran out");
    }
    if passes.unknown > 0 {
        return not_stopped("a process's user couldn't be read, so it may be a pool user's");
    }
    if let Some(why) = passes.mark_failed {
        return Err((StopExit::NotMarked, format!("{counts}; {why}")));
    }
    let retried = passes.retried;
    Ok(format!(
        "the sandbox's stop: {counts}; {} users marked for a profile reset ({retried} retried)",
        passes.marked.len()
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

/// Step 4: passes until a snapshot finds no pool user's process, or until [`PASS_LIMIT`] has
/// passed, counted in `passes`. Each pass marks the pool users it found that aren't yet
/// (step 6), suspends every pool user's process it found, then terminates each, waiting up to
/// [`EXIT_WAIT`] for each to exit, and no more than [`LAST_WAITS`] past the limit all
/// together; one past the limit stops nothing more and counts what it found as left. A
/// snapshot that can't be taken is the error.
fn run_passes(fs: &mut impl Stopper, pool: &[Sid], passes: &mut Passes) -> Result<(), String> {
    let began = fs.elapsed();
    loop {
        let seen = fs
            .snapshot()
            .map_err(|why| format!("a process snapshot failed: {why}"))?;
        passes.taken += 1;
        passes.list_failed |= seen.iter().any(|process| process.list_failed);
        let own = fs.own_pid();
        let picked = pick(&seen, pool, own);
        passes.unknown = picked.unknown;
        if picked.unknown > 0 {
            passes.suspects.extend(pool.iter().cloned());
        }
        passes
            .found
            .extend(picked.stop.iter().map(|(_, user)| user.clone()));
        mark_new(fs, passes);
        if picked.stop.is_empty() {
            return Ok(());
        }
        if fs.elapsed().saturating_sub(began) >= PASS_LIMIT {
            passes.left = picked.stop.len();
            return Ok(());
        }
        // All of them first, so none can start another while the others are terminated; one
        // that won't suspend is still terminated.
        for (pid, _) in &picked.stop {
            drop(fs.suspend(*pid));
        }
        let mut refused = false;
        for (pid, _) in &picked.stop {
            let past = fs.elapsed().saturating_sub(began);
            let wait = EXIT_WAIT.min((PASS_LIMIT + LAST_WAITS).saturating_sub(past));
            match fs.terminate(*pid, wait) {
                Ok(()) => passes.stopped += 1,
                Err(_why) => {
                    passes.retried += 1;
                    refused = true;
                }
            }
        }
        if refused {
            fs.pause();
        }
    }
}

/// Step 5: every BITS job a pool user owns is cancelled, its owner added to `passes.found`;
/// another user's is left alone. It runs whatever the passes found: a pool user's job outlives
/// its processes, and the session's end leaves nothing else to stop. A list that can't be read is
/// the error; a cancel that fails, or an owner that can't be read, is counted.
fn cancel_jobs(fs: &mut impl Stopper, pool: &[Sid], passes: &mut Passes) -> Result<(), String> {
    let owners = fs
        .bits_jobs()
        .map_err(|why| format!("the BITS jobs couldn't be listed: {why}"))?;
    for (at, owner) in owners.iter().enumerate() {
        match owner {
            Some(owner) if pool.contains(owner) => {
                passes.found.insert(owner.clone());
                match fs.cancel_job(at) {
                    Ok(()) => passes.jobs_cancelled += 1,
                    Err(_why) => passes.jobs_failed += 1,
                }
            }
            Some(_) => {}
            None => passes.jobs_failed += 1,
        }
    }
    Ok(())
}

/// Step 6: the users of `passes.found` and `passes.suspects` not yet marked are added to
/// `ResetSids` under the set-up lock, and to `passes.marked`. Nothing is done when there are
/// none, or once a mark has failed (`passes.mark_failed`), so the lock's wait comes once at most.
fn mark_new(fs: &mut impl Stopper, passes: &mut Passes) {
    if passes.mark_failed.is_some() {
        return;
    }
    let wanted = passes.found.union(&passes.suspects);
    let new: Vec<Sid> = wanted
        .filter(|user| !passes.marked.contains(*user))
        .cloned()
        .collect();
    if new.is_empty() {
        return;
    }
    let marked = match fs.lock() {
        Ok(true) => {
            let marked = fs.mark_for_reset(&new);
            fs.unlock();
            marked
        }
        Ok(false) => Err("the set-up lock stayed held".to_owned()),
        Err(why) => Err(why),
    };
    match marked {
        Ok(()) => passes.marked.extend(new),
        Err(why) => passes.mark_failed = Some(format!("not every user was marked: {why}")),
    }
}
