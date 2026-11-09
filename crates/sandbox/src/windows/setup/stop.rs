//! `clauseo-sandbox-setup.exe --stop` (0049, "Stopping the pool's processes"), run by the SYSTEM
//! task `\Clauseo\SandboxStop`: stops every process whose user is a pool user, whatever its DACL
//! says. The runner's sweeps stop a matter's processes as the pool user; a process that
//! shut its own DACL survives them and could stay alive into the next logon. SYSTEM, with
//! `SeDebugPrivilege`, opens it regardless.
//!
//! [`stop`] holds the order and the refusals, with no Windows call; every effect goes through
//! [`Stopper`], which `windows_stopper.rs` implements and the tests fake. Steps 1 to 3 are the
//! gate and step 5 the BITS step that `--sweep-tasks` shares (`pool_steps.rs`). It:
//!
//! 1. refuses unless this process is LOCAL SYSTEM and runs from the admin copy's path;
//! 2. reads the pool from the machine's record, `HKLM\SOFTWARE\Clauseo\Sandbox`: `PoolSids` and
//!    `OwnerSid`, which only the elevated set-up writes. Nothing else names a SID: no argument
//!    (the task passes `--stop` alone, `args.rs`), no environment, nothing under the lawyer's
//!    data folder. No record, or an empty pool, is nothing to stop;
//! 3. holds every SID to the pool-user check (`strays::check_pool_sid`, as `processes.rs` does
//!    before a profile delete): a local account of this machine, not built in, not SYSTEM, not
//!    the owner, named `ClauseoSbNN` when it resolves. One that fails stops nothing;
//!    3a. reserves this run's number, before its first process snapshot ([`sequence::reserve`]):
//!    under the set-up lock (a bounded wait), it checks the record still holds the pool, reads
//!    `StopNext` (1 when absent), keeps it as `n` and writes it one higher. A record gone or
//!    changed, or a write that fails, is 50 and a read that fails 46, with nothing stopped. A
//!    lock that stays held doesn't stop the kill: the run goes on without a number, owes its
//!    users' resets in `ResetOwed` ([`owed`], step 6) and writes nothing else to the record (no
//!    completion), ending 50 at best, so a set-up holding the lock can't keep an escapee alive.
//!    The task runs one instance at a time, so no two reservations overlap;
//! 4. passes over the processes until one finds none of the pool's: each takes a snapshot and
//!    reads each process's user ([`Process`]) from the kernel's process list
//!    (`WTSEnumerateProcessesExW`), with the token's user as a cross-check. The list reads the
//!    token too: a process whose token's DACL shuts out SYSTEM gets no SID from it (CI run
//!    42416074600), and the token can't be read either, so its owner names it (below), read
//!    under `SeDebugPrivilege`, which passes the process's DACL, not the token's. It picks each
//!    process whose user is exactly one of the pool's SIDs ([`pick`]; never this process, pid 0
//!    or pid 4), and marks each pool user it finds for a reset at once (step 6), before anything
//!    is stopped. It then suspends them all,
//!    and terminates each through the handle its user was read through, so a reused pid can't
//!    redirect it and a parent can't start another between two terminations. The passes stop
//!    once one finds no pool user's process, or after [`PASS_LIMIT`]; what is still there then
//!    is left (48). Once they stop, it requests `\Clauseo\SandboxSweep`
//!    ([`Stopper::request_sweep`], logged, never an exit): the escapee it killed may have
//!    registered a task or a BITS job, and a marker it held re-marks to a sweep whose number is
//!    read once this stop completed. A process whose user neither the list nor the token gives is
//!    unknown (48) and never stopped, with two exceptions. Its owner is a pool SID: it is stopped
//!    (the owner may add a process, never clear one: an owner is its creator, not its user, and an
//!    elevated administrator's processes are owned by Administrators). Or it is one of the kernel's
//!    minimal processes: it can't be opened to be terminated, the list put it in session 0, it has
//!    no image file and it isn't a subsystem (WSL 1 pico) process ([`kernel_made`]). Any image at
//!    all makes it unknown, wherever it lies and whoever signed it: a pool user's scheduled task
//!    (S4U, or "run whether logged on or not") reaches session 0 and can start a Windows-signed
//!    System32 binary as a protected process (PPL), which SYSTEM can open with neither
//!    `READ_CONTROL` nor `PROCESS_TERMINATE`, and whose token's DACL can shut SYSTEM out of the
//!    list. SYSTEM can't kill it, so the answer is 48 and a reset, not a kill. A list that couldn't
//!    be read at all leaves no exemption, so a run where it failed ends at 48 unless each process's
//!    token, or a pool owner, named it. A run that ends 48 with unknown processes lists their image
//!    file names in its log line, never a folder or a SID;
//! 5. cancels every BITS job a pool user owns (`BG_JOB_ENUM_ALL_USERS`, each job's owner), even
//!    when no process was found: a pool user's job doesn't run, but stays queued until it is
//!    cancelled. One that can't be cancelled, or whose owner can't be read, is 48; so is a list
//!    of [`JOBS_CAP`] jobs, the most read, as more may be past it (the pool's jobs within it are
//!    cancelled first, and the log line says the list was longer);
//!    5b. ends each pool user's WSL 2 VM ([`vms`]): of HCS's compute systems owned by `WSL`,
//!    one whose folder, `temp\<id>` under `Local AppData`, is in the lawyer's or any other
//!    non-pool profile is left alone (checked first); one whose folder is in a pool user's
//!    profile is terminated, with a bounded wait, and its user marked; one in no profile marks
//!    every pool user and is terminated too, unless a non-pool profile couldn't be read, when
//!    it is unmapped, as is one a non-pool profile may hold. With no HCS (`computecore.dll` or
//!    the `vmcompute` service missing) there is nothing to do. A VM that isn't ended, an
//!    unmapped WSL system, or a list that fails while `WSLService` or `LxssManager` is
//!    installed, is 48. The
//!    pool's WSL 1 instances need nothing more: their pico processes, `wslhost.exe` and
//!    `wslrelay.exe` carry the pool user's token, and step 4 stops them;
//! 6. adds each pool user it found running, owning a job or a VM, to the record's `ResetSids`,
//!    under the set-up lock (a bounded wait), so the next set-up deletes and remakes its profile
//!    before the user runs anything again (`resets.rs`): what the stopped process left in its
//!    profile goes with it, WSL's distro registrations too (they are in the user's hive).
//!    Each user is added as soon as it is found, so a run the task's 2-minute limit kills has
//!    marked every user it saw. A pass that finds a process of unknown user marks every
//!    pool user at once, before anything is stopped, as that process may be any of theirs. Once
//!    the lock stays held (or step 3a's did), this and every later mark of the run goes to
//!    `ResetOwed` instead, without the lock ([`owed::owe`]), so the lock's wait comes once at
//!    most and a reset is owed for every user the run found; such a run doesn't try step 7 and
//!    ends 49 (50 without a number). A write that fails, a record that no longer holds the
//!    pool, or `ResetOwed` not written stops the marks (49);
//! 7. only then, on a run that will end 0, under the set-up lock ([`sequence::completed_after`],
//!    `record_values::STOP_NEXT`), once it has checked the record still holds the pool this run
//!    worked from (the same set of SIDs) and that `StopSequence` isn't already past `n`, folds
//!    `ResetOwed` (its own and earlier runs') into `ResetSids` and deletes it ([`owed::fold`]),
//!    then writes `StopCompleted`, then `StopSequence = n`. `StopCompleted` is when the last clean
//!    observation began, not when it is written: the earlier of the pass that found no pool user's
//!    process (after which nothing was stopped) and the BITS step's enumeration, so a pool process
//!    started after it isn't claimed covered. A run the task's end kills, or one that ends
//!    non-zero, never writes them and leaves an earlier pair as it is (`StopNext` still counted
//!    up). A record gone or changed, a sequence past `n`, a write that fails, or a lock that stays
//!    held is 49. Every write opens the record's key without making it, so a stop still running
//!    after `--remove` can't bring the key back.
//!
//! The owner may end the task: it holds read and execute on it (`FRFX`, `refresh::task_sddl`),
//! since starting a task needs execute (CI run 42416074600), and execute ends one too. So an
//! ended stop must never pass for a finished one: only a completed stop writes `StopSequence`.
//! The rule the follow-up PR keeps: its unclean-session marker records `StopNext` as it reads it
//! when written (M), reading it under the set-up lock so it can't interleave with a
//! reservation's read-then-write and record a stale M. The logon refusal (F4) lifts, and the
//! marker moves on to a sweep whose number is read then, only when `StopSequence >= M`: a stop
//! reserved its number after the marker existed and then completed, so its observation began
//! after the marker. Nothing trusts the clock;
//! `StopCompleted` is for diagnostics. `--remove` deletes `StopNext`, `StopSequence` and
//! `StopCompleted` with the record, and should clear the markers, so no marker outlives its
//! set-up. Ending a stop is denial of service only, which code running as the owner could do to
//! its own sessions anyway.
//!
//! The reset itself isn't done here: it sets a new password, which only the owner's
//! `secrets.bin` may hold (DPAPI, as the owner), and its open-session check and its `LocalLow`
//! denies read and write the owner's data folder, as the owner. SYSTEM has no owner token here,
//! so the reset waits for the next set-up.
//!
//! It logs counts only, one line, and exits with its own code ([`StopExit`]).

use std::collections::BTreeSet;
use std::time::Duration;

use super::pool_steps::sequence::{self, NotReserved, STOP};
use super::pool_steps::{Bits, Gate, JOBS_CAP, Record, Refused, cancel_pool_jobs, gate};
use crate::windows::sids::Sid;

pub use super::pool_steps::{Failed, Pool};
pub use pick::{Image, Picked, Process, image_names, kernel_made, pick, wsl_image};
pub use vms::{
    LIST_WAIT, VM_WAIT, VMS_BUDGET, VmId, VmStopper, WSL_OWNER, local_app_data, wsl_ids_from_json,
};

mod owed;
mod pick;
#[cfg(test)]
mod tests;
mod vms;
#[cfg(windows)]
mod windows_hcs;
#[cfg(windows)]
mod windows_profiles;
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
    /// 0: every pool user's process found was stopped (or none ran), those users are marked, and
    /// `StopCompleted` and `StopSequence` (the reserved `n`) are written (with a pool recorded;
    /// with none there is nothing to clear).
    Done,
    /// 45: not LOCAL SYSTEM, or not run from the admin copy's path; nothing was stopped.
    NotAllowed,
    /// 46: the record can't be read, or its `PoolSids`, `OwnerSid` or `StopNext` is malformed;
    /// nothing was stopped.
    NoRecord,
    /// 47: a SID `PoolSids` lists isn't a pool user's; nothing was stopped.
    NotPool,
    /// 48: a pool user's process was still there when the passes ran out of time, a process's
    /// user couldn't be read, a pool user's BITS job wasn't cancelled (or a job's owner couldn't
    /// be read), a process snapshot or the BITS list couldn't be taken, the BITS list reached
    /// [`JOBS_CAP`], or a pool user's WSL VM wasn't ended (or may be one, or HCS's list failed
    /// with WSL installed).
    NotStopped,
    /// 49: the processes were stopped, but their users couldn't be marked in `ResetSids` (a
    /// write failed or the record was gone or its pool changed) nor owed in `ResetOwed`, or
    /// `ResetOwed` couldn't be folded, or `StopCompleted` and `StopSequence` couldn't be
    /// written (the set-up lock stayed held, a write failed, the record was gone or its pool
    /// changed, or `StopSequence` was past `n`).
    NotMarked,
    /// 50: `StopNext` couldn't be reserved before the first snapshot: the record was gone or its
    /// pool changed, or the write failed (nothing was stopped), or the set-up lock stayed held
    /// (the processes were still stopped, the jobs cancelled and the users owed a reset in
    /// `ResetOwed`, but nothing else was written to the record). 48 wins over it.
    NotReserved,
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
            StopExit::NotReserved => 50,
        }
    }
}

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

/// Every effect of the stop: the gate's ([`Gate`]), the BITS step's ([`Bits`]), the record's
/// ([`Record`]: the set-up lock, its values, the wall clock) and its own. The
/// real one reads the record and the admin copy's path itself, so no method takes a SID or a
/// path the caller chose, but the pool the record gave. A list of [`JOBS_CAP`] BITS jobs may
/// have been cut, so jobs of the pool's may be left past it: the run ends at 48. The WSL step's
/// are [`VmStopper`]'s.
pub trait Stopper: Gate + Bits + Record + VmStopper {
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
    /// Adds `users` to the record's `ResetSids`, keeping what it lists.
    fn mark_for_reset(&mut self, users: &[Sid]) -> Result<(), Failed>;
    /// The record's `ResetOwed`; empty when it is missing. Read with or without the lock.
    fn read_owed(&mut self) -> Result<Vec<Sid>, Failed>;
    /// Writes the record's `ResetOwed` as `users`, deleting it when there are none; never
    /// makes the record's key.
    fn write_owed(&mut self, users: &[Sid]) -> Result<(), Failed>;
    /// Asks the Task Scheduler to run `\Clauseo\SandboxSweep`; `Err` names the failed call.
    fn request_sweep(&mut self) -> Result<(), Failed>;
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
    /// Their image file names ([`Image::file_name`]).
    unknown_images: Vec<String>,
    /// A snapshot's process list (`WTSEnumerateProcessesExW`) couldn't be read.
    list_failed: bool,
    /// Every pool user, once a pass found a process of unknown user, which may be any of
    /// theirs: marked as `found` is, but not counted as found running.
    suspects: BTreeSet<Sid>,
    /// The users of `found` and `suspects` added to `ResetSids`.
    marked: BTreeSet<Sid>,
    /// Why a mark failed; once set, no more are tried.
    mark_failed: Option<String>,
    /// The set-up lock stayed held: marks go to `ResetOwed`, without it.
    lockless: bool,
    /// The pool users' BITS jobs cancelled.
    jobs_cancelled: usize,
    /// BITS jobs not cancelled: a pool user's whose cancel failed, or one whose owner couldn't
    /// be read.
    jobs_failed: usize,
    /// The BITS list reached [`JOBS_CAP`], so jobs past it may not have been read.
    jobs_capped: bool,
    /// When the last pass began (`Stopper::now`) whose snapshot found no pool user's process.
    clean_at: Option<u64>,
    /// When the BITS step's enumeration began.
    jobs_at: Option<u64>,
    /// What the WSL step did.
    vms: vms::Vms,
    /// Evidence that a pool user started WSL, recorded before any process is stopped
    /// ([`vms::evidence`], and each pass's snapshot): without it the WSL step is skipped.
    wsl_evidence: bool,
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
    // Steps 1 to 3, shared with `--sweep-tasks`.
    let pool = match gate(fs) {
        Ok(Some(pool)) => pool,
        Ok(None) => return Ok("the sandbox's stop: no pool users are recorded".to_owned()),
        Err((Refused::NotAllowed, why)) => return Err((StopExit::NotAllowed, why)),
        Err((Refused::NoRecord, why)) => return Err((StopExit::NoRecord, why)),
        Err((Refused::NotPool, why)) => {
            return Err((StopExit::NotPool, format!("nothing was stopped: {why}")));
        }
    };
    // A lock that stays held stops nothing less: the run goes on, owing its resets.
    let (n, unreserved) = match sequence::reserve(fs, STOP, &pool.users) {
        Ok(n) => (n, None),
        Err(NotReserved::Lock(why)) => (0, Some(why)),
        Err(why) => return Err(not_reserved(why)),
    };
    let mut passes = Passes {
        lockless: unreserved.is_some(),
        ..Passes::default()
    };
    vms::evidence(fs, &pool.users, &mut passes);
    let ran = run_passes(fs, &pool.users, &mut passes);
    // What the processes left (a task, a job) is the sweep's: asked for once the kills are done.
    let requested = match fs.request_sweep() {
        Ok(()) => "; the sweep task was requested".to_owned(),
        Err(why) => format!("; the sweep task couldn't be requested ({why})"),
    };
    let cancelled = cancel_jobs(fs, &pool.users, &mut passes);
    let vms = vms::stop_vms(fs, &pool, &mut passes);
    // Without the list, a process no token names is unknown: the line says why.
    let list = if passes.list_failed {
        " (the process list couldn't be read)"
    } else {
        ""
    };
    // File names only: the Application log is readable by every local user.
    let images = if passes.unknown_images.is_empty() {
        String::new()
    } else {
        format!("; unknown images: {}", image_names(&passes.unknown_images))
    };
    let capped = if passes.jobs_capped {
        format!(" (the BITS job list was longer than {JOBS_CAP})")
    } else {
        String::new()
    };
    let counts = format!(
        "{} pool users, {} found running, {} processes stopped in {} passes, {} left, \
         {} unknown{list}, {} BITS jobs cancelled, {} not{capped}{}{images}{requested}",
        pool.users.len(),
        passes.found.len(),
        passes.stopped,
        passes.taken,
        passes.left,
        passes.unknown,
        passes.jobs_cancelled,
        passes.jobs_failed,
        passes.vms.counts()
    );
    mark_new(fs, &pool.users, &mut passes);
    let not_stopped = |why: &str| Err((StopExit::NotStopped, format!("{counts}; {why}")));
    if let Err(why) = ran.and(cancelled).and(vms) {
        return not_stopped(&why);
    }
    if passes.jobs_failed > 0 || passes.jobs_capped {
        return not_stopped(
            "a BITS job wasn't cancelled, its owner couldn't be read, or the list was cut",
        );
    }
    if passes.vms.left > 0 || passes.vms.unmapped > 0 {
        return not_stopped("a pool user's WSL VM wasn't ended, or a WSL VM's user isn't known");
    }
    if passes.left > 0 {
        return not_stopped("a pool user's process was still there when the passes' time ran out");
    }
    if passes.unknown > 0 {
        return not_stopped("a process's user couldn't be read, so it may be a pool user's");
    }
    if let Some(why) = unreserved {
        let line = format!(
            "{counts}; nothing but ResetOwed was written to the record: {UNRESERVED}: {why}"
        );
        return Err((StopExit::NotReserved, line));
    }
    if let Some(why) = passes.mark_failed {
        return Err((StopExit::NotMarked, format!("{counts}; {why}")));
    }
    // The lock's wait came once: the completion isn't tried after it.
    if passes.lockless {
        let why = "the set-up lock stayed held: the users found are owed a reset in ResetOwed";
        return Err((StopExit::NotMarked, format!("{counts}; {why}")));
    }
    // The earlier clean observation: nothing the record claims covered started before it.
    let clean = passes.clean_at.into_iter().chain(passes.jobs_at).min();
    let Some(at) = clean else {
        return not_stopped("no pass found the pool clear");
    };
    // Every owed reset is in ResetSids before StopSequence clears a marker.
    sequence::completed_after(fs, STOP, (at, n), &pool.users, owed::fold)
        .map_err(|why| (StopExit::NotMarked, format!("{counts}; {why}")))?;
    let retried = passes.retried;
    Ok(format!(
        "the sandbox's stop: {counts}; {} users marked for a profile reset ({retried} retried)",
        passes.marked.len()
    ))
}

/// Step 3a's failure: nothing was stopped. A lock that stays held, a record that no longer holds
/// the pool, or a write that fails is [`StopExit::NotReserved`]; a read that fails, or a
/// `StopNext` at its end, is [`StopExit::NoRecord`].
fn not_reserved(why: NotReserved) -> Refusal {
    let (exit, why) = match why {
        NotReserved::Lock(why) | NotReserved::Changed(why) => (StopExit::NotReserved, why),
        NotReserved::Unread(why) => (StopExit::NoRecord, why),
    };
    let line = format!("nothing was stopped: {UNRESERVED}: {why}");
    (exit, line)
}

/// What a stop whose number wasn't reserved says.
const UNRESERVED: &str = "StopNext wasn't reserved";

/// Step 4: passes until a snapshot finds no pool user's process, or until [`PASS_LIMIT`] has
/// passed, counted in `passes`. Each pass marks the pool users it found that aren't yet
/// (step 6), suspends every pool user's process it found, then terminates each, waiting up to
/// [`EXIT_WAIT`] for each to exit, and no more than [`LAST_WAITS`] past the limit all
/// together; one past the limit stops nothing more and counts what it found as left. A
/// snapshot that can't be taken is the error.
fn run_passes(fs: &mut impl Stopper, pool: &[Sid], passes: &mut Passes) -> Result<(), String> {
    let began = fs.elapsed();
    loop {
        let at = fs.now();
        let seen = fs
            .snapshot()
            .map_err(|why| format!("a process snapshot failed: {why}"))?;
        passes.taken += 1;
        passes.list_failed |= seen.iter().any(|process| process.list_failed);
        let own = fs.own_pid();
        let picked = pick(&seen, pool, own);
        passes.unknown = picked.unknown;
        passes.unknown_images = picked.unknown_images;
        if picked.unknown > 0 {
            passes.suspects.extend(pool.iter().cloned());
        }
        passes
            .found
            .extend(picked.stop.iter().map(|(_, user)| user.clone()));
        // A pool user's WSL process is evidence for the WSL step, recorded before it is stopped.
        let wsl = |pid: &u32| {
            seen.iter()
                .any(|process| process.pid == *pid && process.wsl)
        };
        passes.wsl_evidence |= picked.stop.iter().any(|(pid, _)| wsl(pid));
        mark_new(fs, pool, passes);
        if picked.stop.is_empty() {
            passes.clean_at = Some(at);
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

/// Step 5: every BITS job a pool user owns is cancelled ([`cancel_pool_jobs`]), its owner added
/// to `passes.found`; another user's is left alone. It runs whatever the passes found: a pool
/// user's job outlives its processes, and the session's end leaves nothing else to stop. A list
/// that can't be read is the error; a cancel that fails, or an owner that can't be read, is
/// counted; a list of [`JOBS_CAP`] jobs is marked `jobs_capped` once the pool's jobs in it are
/// cancelled.
fn cancel_jobs(fs: &mut impl Stopper, pool: &[Sid], passes: &mut Passes) -> Result<(), String> {
    passes.jobs_at = Some(fs.now());
    let jobs = cancel_pool_jobs(fs, pool)?;
    passes.found.extend(jobs.owners);
    passes.jobs_cancelled = jobs.cancelled;
    passes.jobs_failed = jobs.failed;
    passes.jobs_capped = jobs.capped;
    Ok(())
}

/// Step 6: the users of `passes.found` and `passes.suspects` not yet marked are added to
/// `ResetSids` under the set-up lock, and to `passes.marked`, once the record is checked to
/// still hold `pool` (a set-up may have changed it since the gate). Once the lock stays held
/// (`passes.lockless`), they are owed in `ResetOwed` instead, without it. Nothing is done when
/// there are none, or once a mark has failed (`passes.mark_failed`), so the lock's wait comes
/// once at most.
fn mark_new(fs: &mut impl Stopper, pool: &[Sid], passes: &mut Passes) {
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
    let marked = if passes.lockless {
        owed::owe(fs, &new)
    } else if let Ok(true) = fs.lock() {
        let marked = still_holds(fs, pool).and_then(|()| fs.mark_for_reset(&new));
        fs.unlock();
        marked
    } else {
        // Owed without the lock, as every later mark of the run is.
        passes.lockless = true;
        owed::owe(fs, &new)
    };
    match marked {
        Ok(()) => passes.marked.extend(new),
        Err(why) => passes.mark_failed = Some(format!("not every user was marked: {why}")),
    }
}

/// Whether the record, read again under the lock, still holds `pool`.
fn still_holds(fs: &mut impl Stopper, pool: &[Sid]) -> Result<(), Failed> {
    let now = fs.pool()?.map(|pool| pool.users);
    if sequence::holds(now, pool) {
        Ok(())
    } else {
        Err("the record is gone, or its pool changed since the run began".to_owned())
    }
}
