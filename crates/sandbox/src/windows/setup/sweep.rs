//! `clauseo-sandbox-setup.exe --sweep-tasks`, run by the SYSTEM task `\Clauseo\SandboxSweep`,
//! which the runner asks for at every sandbox session's end: deletes every scheduled task whose
//! principal is a pool user and cancels every BITS job a pool user owns. Either would let a
//! pool user run something after its session: a task at its trigger, a job once its owner logs
//! on again. It replaces the runner's end sweeper, which ran as each pool user. It kills
//! nothing: stopping a pool user's processes is `--stop`'s (`stop.rs`), and [`Sweeper`] has no
//! process call at all; a pool task's running instance is stopped through the Task Scheduler.
//!
//! [`sweep`] holds the order and the refusals, with no Windows call; every effect goes through
//! [`Sweeper`], which `windows_sweeper.rs` implements and the tests fake. Steps 1 to 3 are the
//! gate `--stop` uses and step 5 its BITS step (`pool_steps.rs`). It:
//!
//! 1. refuses unless this process is LOCAL SYSTEM and runs from the admin copy's path (53);
//! 2. reads the pool from the machine's record, `HKLM\SOFTWARE\Clauseo\Sandbox`: `PoolSids` and
//!    `OwnerSid`, which only the elevated set-up writes. Nothing else names a SID: no argument
//!    (the task passes `--sweep-tasks` alone, `args.rs`), no environment, nothing under the
//!    lawyer's data folder. A record that can't be read is 54; no record, or an empty pool, is
//!    nothing to sweep (0);
//! 3. holds every SID to the pool-user check (`processes::check_pool`): a local account of this
//!    machine, not built in, not SYSTEM, not the owner. One that fails touches nothing (55).
//!    Then it reserves this run's number before its walk, as `--stop` does
//!    (`pool_steps::sequence`): under the set-up lock, it checks the record still holds the
//!    pool, reads `SweepNext` (1 when absent), keeps it as `n` and writes it one higher. A
//!    record gone or changed, or a write that fails, is 58 with nothing touched; a read that
//!    fails is 54. A lock that stays held still lets the run sweep, but it then records nothing
//!    and ends 58 at best;
//! 4. walks every Task Scheduler folder from `\`, each folder's subfolders too, with no depth
//!    limit but at most [`FOLDERS_CAP`] folders. In each it reads every task, hidden ones
//!    included, and its definition's principal: `UserId`, or `GroupId` when that is empty. It
//!    resolves that name ([`resolve`]): a SID's text in any form Windows takes (`s-`, a hex
//!    authority, leading zeros) is that SID ([`canonical_sid`]); the Task Scheduler's own names
//!    `LocalSystem`, `LocalService` and `NetworkService` (any case) are S-1-5-18, -19 and -20;
//!    any other name is looked up (`LookupAccountNameW`, which finds a local account on this
//!    machine). It deletes the task when that SID is exactly one of the pool's (equal SIDs,
//!    never a prefix or a domain), or when the owner its security descriptor names is
//!    ([`sddl_owner`]), whatever its principal. A pool principal is always a local account
//!    (`ClauseoSbNN`) or a SID's text, and both resolve here, so a name that resolves to no SID
//!    is the pool's when its account part is a pool user's name, without case, with no prefix
//!    or one of `.\` and this computer's names ([`Names::is_pool_name`]); it is then deleted.
//!    Such a name under another domain's prefix ([`Names::is_foreign`]: a domain account with
//!    no domain controller reachable) is skipped, counted in the log line (never named), and
//!    not 56. Any other such name (a deleted account, say), a task whose principal names
//!    neither a user nor a group, a task whose principal or owner can't be read, a
//!    folder whose tasks or subfolders can't be listed, a delete that fails, and more than
//!    [`FOLDERS_CAP`] folders are 56, and none stops the walk: every other task is still read,
//!    and every pool task still deleted. Before a pool task is deleted, its running instances
//!    are stopped ([`running`]: `GetRunningTasks`, each instance whose path is the task's,
//!    `IRunningTask::Stop`), since deleting a task ends nothing; one not stopped is 56, and the
//!    task is deleted all the same. A task whose definition or principal the Task Scheduler
//!    won't give (its descriptor shuts SYSTEM out, say) is first held to its owner: a pool SID
//!    is the pool's task, deleted as above; an owner that can't be read is 56. Otherwise it is
//!    read from its file, `%SystemRoot%\System32\Tasks\<folder>\<name>` ([`task_file`]):
//!    each part a plain name, opened by handle below `Tasks`, never through a link, with
//!    `SeBackupPrivilege` for the opens alone and no right to write asked, at most
//!    [`TASK_FILE_CAP`] bytes, its XML read narrowly, its owner and DACL read through the same
//!    handle ([`file_trust`]). Its creator may hold write on it, so the file counts only when
//!    its owner isn't a pool SID and no allow in its DACL gives a right to change it (write,
//!    append, extended attributes, attributes, delete, `WRITE_DAC`, `WRITE_OWNER`, generic
//!    write or all) to anyone a pool user's token may hold: every SID but SYSTEM,
//!    Administrators, the service accounts, a service's SID, CREATOR OWNER, OWNER RIGHTS and
//!    another domain's accounts and groups; a NULL DACL lets everyone in. When such a file's
//!    principal resolves to a SID that isn't the pool's, or is another domain's name, the task
//!    is skipped and counted; when it is the pool's or resolves to nothing else, or the file
//!    can't be read or trusted, the task is unreadable (56) and left;
//! 5. cancels every BITS job a pool user owns, whatever step 4 found (`BG_JOB_ENUM_ALL_USERS`,
//!    each job's owner). One that can't be cancelled, an owner that can't be read, a list that
//!    can't be read, or one of `JOBS_CAP` jobs is 57; 56 wins when both;
//! 6. only on a run that will end 0, under the set-up lock, checks the record still holds the
//!    pool and writes `SweepCompleted` (when the walk began) and then `SweepSequence = n`; a
//!    `SweepSequence` already past `n`, a record gone or changed, a write that fails or a lock
//!    that stays held is 59. A run that is ended or ends non-zero never writes them. A session's
//!    unclean-session marker waits for `SweepSequence` to reach the `SweepNext` it read
//!    (`runner::marker`);
//! 7. logs one line with counts only: no path, task name or SID, as the Application log is
//!    readable by every local user.
//!
//! It marks no profile for a reset, unlike `--stop`.

use std::collections::BTreeMap;

use super::pool_steps::sequence::{self, NotReserved, SWEEP};
use super::pool_steps::{
    Bits, Failed, Gate, JOBS_CAP, Jobs, Record, Refused, cancel_pool_jobs, gate,
};
use crate::windows::sids::Sid;

mod file_trust;
mod resolve;
mod running;
pub mod task_file;
#[cfg(test)]
mod tests;
#[cfg(windows)]
pub(crate) mod windows_sweeper;
#[cfg(windows)]
mod windows_task_file;
#[cfg(windows)]
#[cfg(test)]
mod windows_tests;

#[cfg(any(windows, test))]
pub use file_trust::file_security;
pub use file_trust::{FileSecurity, TaskFile, WRITE_RIGHTS, may_be_pool, trusted};
use resolve::pool_owned;
pub use resolve::{Names, Resolved, canonical_sid, resolve, sddl_owner};
pub use task_file::TASK_FILE_CAP;

/// The sweep's exit codes. They start at 53, after [`StopExit`]'s 50 and the WSL deny task's 51
/// and 52 (`WslDenyExit`), so the one exe never gives one number two meanings; codes are
/// appended from here on, never renumbered.
///
/// [`StopExit`]: super::stop::StopExit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepExit {
    /// 0: every pool user's task found was deleted and every pool user's BITS job cancelled,
    /// every task and folder read (with a pool recorded; with none there is nothing to sweep).
    Done,
    /// 53: not LOCAL SYSTEM, or not run from the admin copy's path; nothing was touched.
    NotAllowed,
    /// 54: the record can't be read, or its `PoolSids` or `OwnerSid` is malformed; nothing was
    /// touched.
    NoRecord,
    /// 55: a SID `PoolSids` lists isn't a pool user's; nothing was touched.
    NotPool,
    /// 56: a task's principal or owner couldn't be read, or names no one, or a name that
    /// resolves to no SID and isn't another domain's (a task file read instead whose owner or
    /// DACL lets a pool user change it counts as unread); a pool user's task couldn't be
    /// deleted, or a running instance of one couldn't be stopped (or its path read), a folder's
    /// tasks or subfolders couldn't be listed, or there were more than [`FOLDERS_CAP`] folders.
    /// The BITS step still ran.
    TasksLeft,
    /// 57: every task was read and the pool's deleted, but a pool user's BITS job wasn't
    /// cancelled, a job's owner couldn't be read, or the BITS list couldn't be read or reached
    /// `JOBS_CAP`.
    JobsLeft,
    /// 58: `SweepNext` couldn't be reserved before the walk: the record was gone or its pool
    /// changed, or the write failed (nothing was touched), or the set-up lock stayed held (the
    /// tasks and jobs were still swept, but nothing was recorded). 56 and 57 win over it.
    NotReserved,
    /// 59: the run would have ended 0, but `SweepCompleted` and `SweepSequence` couldn't be
    /// written (the set-up lock stayed held, a write failed, the record was gone or its pool
    /// changed, or `SweepSequence` was past `n`).
    NotRecorded,
}

impl SweepExit {
    /// The process exit code.
    pub fn as_code(self) -> u8 {
        match self {
            SweepExit::Done => 0,
            SweepExit::NotAllowed => 53,
            SweepExit::NoRecord => 54,
            SweepExit::NotPool => 55,
            SweepExit::TasksLeft => 56,
            SweepExit::JobsLeft => 57,
            SweepExit::NotReserved => 58,
            SweepExit::NotRecorded => 59,
        }
    }
}

/// The most Task Scheduler folders the walk reads: one more left to read is 56.
pub const FOLDERS_CAP: usize = 10_000;

/// A task's principal as its definition gives it; either may be empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Principal {
    /// `IPrincipal::UserId`.
    pub user_id: String,
    /// `IPrincipal::GroupId`.
    pub group_id: String,
}

impl Principal {
    /// The name the task runs as: `UserId`, or `GroupId` when that is empty; `None` when both
    /// are.
    pub fn name(&self) -> Option<&str> {
        [&self.user_id, &self.group_id]
            .into_iter()
            .find(|name| !name.is_empty())
            .map(String::as_str)
    }
}

/// A task as a folder's list gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Its name in its folder, for [`Sweeper::delete_task`]; never logged.
    pub name: String,
    /// Its principal; `Err` when the task, its definition or its principal couldn't be read.
    pub principal: Result<Principal, Failed>,
}

/// Every effect of the sweep: the gate's ([`Gate`]), the BITS step's ([`Bits`]), the record's
/// ([`Record`]) and the Task Scheduler's. No method stops, suspends or opens a process.
pub trait Sweeper: Gate + Bits + Record {
    /// A Task Scheduler folder, as the machine holds it.
    type Folder;
    /// The root folder, `\`.
    fn root(&mut self) -> Result<Self::Folder, Failed>;
    /// The folders directly below `folder`, each `Err` where it couldn't be read; `Err` when
    /// the list couldn't be.
    fn subfolders(
        &mut self,
        folder: &Self::Folder,
    ) -> Result<Vec<Result<Self::Folder, Failed>>, Failed>;
    /// Every task in `folder`, hidden ones included; `Err` when the list couldn't be read.
    fn tasks(&mut self, folder: &Self::Folder) -> Result<Vec<Task>, Failed>;
    /// The SID of the account or group `name` (`LookupAccountNameW`); `Err` when it names none.
    fn lookup_account(&mut self, name: &str) -> Result<Sid, Failed>;
    /// The SDDL of the task `name` in `folder`'s security descriptor, its owner at least
    /// (`IRegisteredTask::GetSecurityDescriptor`); `Err` when it can't be read.
    fn task_sddl(&mut self, folder: &Self::Folder, name: &str) -> Result<String, Failed>;
    /// The name of this machine's local account `sid`, without a domain (`LookupAccountSidW`);
    /// `Err` when no local account has it.
    fn account_name(&mut self, sid: &Sid) -> Result<String, Failed>;
    /// This computer's names: its `NetBIOS` name and its DNS host name, those that can be read.
    fn computer_names(&mut self) -> Vec<String>;
    /// `folder`'s path (`ITaskFolder::Path`): `\` for the root, `\A\B` below it.
    fn folder_path(&mut self, folder: &Self::Folder) -> Result<String, Failed>;
    /// The task file whose path below `%SystemRoot%\System32\Tasks` is `parts` (each a plain
    /// name, [`task_file::parts`]), read as the module says, at most [`TASK_FILE_CAP`] bytes,
    /// with its owner and DACL read through the same handle; `Err` when either can't be read,
    /// or the file is larger.
    fn read_task_file(&mut self, parts: &[String]) -> Result<TaskFile, Failed>;
    /// Every running task instance, hidden ones included
    /// (`ITaskService::GetRunningTasks(TASK_ENUM_HIDDEN)`): each its task's path
    /// (`IRunningTask::Path`), `Err` where it can't be read; `Err` when the list can't be. It
    /// replaces the last list, for [`Sweeper::stop_running`].
    fn running_tasks(&mut self) -> Result<Vec<Result<String, Failed>>, Failed>;
    /// Stops the instance at `at` in the last list (`IRunningTask::Stop`).
    fn stop_running(&mut self, at: usize) -> Result<(), Failed>;
    /// Deletes the task `name` from `folder`.
    fn delete_task(&mut self, folder: &Self::Folder, name: &str) -> Result<(), Failed>;
    /// Writes one line to the log: `error` for a refusal or a failure.
    fn log(&mut self, error: bool, line: &str);
}

/// What the task walk (step 4) did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Swept {
    /// Folders read, or tried.
    pub folders: usize,
    /// Tasks listed.
    pub tasks: usize,
    /// The pool's tasks deleted.
    pub deleted: usize,
    /// Tasks whose principal or owner couldn't be read, and whose task file, when read
    /// instead, didn't show them not the pool's.
    pub unreadable: usize,
    /// Tasks whose principal's name resolves to no SID and is another domain's: skipped.
    pub skipped: usize,
    /// Tasks whose principal's name resolves to no SID, isn't another domain's and isn't a pool
    /// user's.
    pub unresolved: usize,
    /// Tasks whose principal names neither a user nor a group.
    pub no_principal: usize,
    /// The pool's tasks whose delete failed.
    pub not_deleted: usize,
    /// Folders whose tasks or subfolders couldn't be listed, a subfolder that couldn't be
    /// read, or the root that couldn't be opened.
    pub unlisted: usize,
    /// More than [`FOLDERS_CAP`] folders: those past it weren't read.
    pub capped: bool,
    /// Each failure's text, with how often it came: never logged, for the tests' diagnostics.
    pub why: BTreeMap<Failed, usize>,
    /// Tasks the Task Scheduler wouldn't give whose task file, its owner and DACL leaving no
    /// pool user a way to change it, names a principal that isn't the pool's: skipped.
    pub file_skipped: usize,
    /// The pool's tasks' running instances stopped.
    pub instances_stopped: usize,
    /// Their instances not stopped, or that may be theirs ([`running`]).
    pub instances_not_stopped: usize,
}

impl Swept {
    /// Whether a task of the pool's may be left: step 4's 56.
    pub fn left(&self) -> bool {
        self.unreadable > 0
            || self.no_principal > 0
            || self.unresolved > 0
            || self.not_deleted > 0
            || self.instances_not_stopped > 0
            || self.unlisted > 0
            || self.capped
    }

    /// Counts `why`.
    fn note(&mut self, why: Failed) {
        *self.why.entry(why).or_default() += 1;
    }
}

/// Runs the sweep as the module says.
pub fn sweep(fs: &mut impl Sweeper) -> SweepExit {
    let pool = match gate(fs) {
        Ok(Some(pool)) => pool,
        Ok(None) => {
            fs.log(
                false,
                "the sandbox's task sweep: no pool users are recorded",
            );
            return SweepExit::Done;
        }
        Err((refused, why)) => {
            let exit = match refused {
                Refused::NotAllowed => SweepExit::NotAllowed,
                Refused::NoRecord => SweepExit::NoRecord,
                Refused::NotPool => SweepExit::NotPool,
            };
            fs.log(
                true,
                &format!("the sandbox's task sweep: nothing was touched: {why}"),
            );
            return exit;
        }
    };
    // Step 3a: a lock that stays held still lets the run sweep, but it records nothing.
    let reserved = match sequence::reserve(fs, SWEEP, &pool.users) {
        Ok(n) => Ok(n),
        Err(NotReserved::Lock(why)) => Err(why),
        Err(NotReserved::Changed(why)) => {
            return refused(fs, SweepExit::NotReserved, &format!("{UNRESERVED}: {why}"));
        }
        Err(NotReserved::Unread(why)) => {
            return refused(fs, SweepExit::NoRecord, &format!("{UNRESERVED}: {why}"));
        }
    };
    let at = fs.now();
    let swept = sweep_tasks(fs, &pool.users);
    let jobs = cancel_pool_jobs(fs, &pool.users);
    let jobs_left = match &jobs {
        Ok(jobs) => jobs.left(),
        Err(_) => true,
    };
    let mut exit = if swept.left() {
        SweepExit::TasksLeft
    } else if jobs_left {
        SweepExit::JobsLeft
    } else {
        SweepExit::Done
    };
    let mut line = log_line(pool.users.len(), &swept, &jobs);
    if exit == SweepExit::Done {
        // Step 6, only on a run that ends 0.
        let recorded = reserved
            .map_err(|why| (SweepExit::NotReserved, format!("{UNRESERVED}: {why}")))
            .and_then(|n| {
                sequence::completed(fs, SWEEP, at, n, &pool.users)
                    .map_err(|why| (SweepExit::NotRecorded, why))
            });
        if let Err((not, why)) = recorded {
            exit = not;
            line = format!("{line}; nothing was recorded: {why}");
        }
    }
    fs.log(exit != SweepExit::Done, &line);
    exit
}

/// What a sweep whose number wasn't reserved says.
const UNRESERVED: &str = "SweepNext wasn't reserved";

/// A refusal after the gate: logged as nothing touched, with `exit`.
fn refused(fs: &mut impl Sweeper, exit: SweepExit, why: &str) -> SweepExit {
    let line = format!("the sandbox's task sweep: nothing was touched: {why}");
    fs.log(true, &line);
    exit
}

/// The run's log line: counts, and the BITS list's failure, which names no user.
fn log_line(users: usize, swept: &Swept, jobs: &Result<Jobs, Failed>) -> String {
    let (cancelled, failed) = jobs
        .as_ref()
        .map_or((0, 0), |jobs| (jobs.cancelled, jobs.failed));
    let unlisted = if swept.unlisted > 0 {
        format!("; {} folders couldn't be listed", swept.unlisted)
    } else {
        String::new()
    };
    let capped = if swept.capped {
        format!("; there were more than {FOLDERS_CAP} folders")
    } else {
        String::new()
    };
    let listed = match jobs {
        Ok(jobs) if jobs.capped => format!("; the BITS job list was longer than {JOBS_CAP}"),
        Ok(_) => String::new(),
        Err(why) => format!("; {why}"),
    };
    format!(
        "the sandbox's task sweep: {users} pool users, {} folders and {} tasks read, {} of the \
         pool's tasks deleted, {} unreadable ({} more skipped, not the pool's by their task \
         file), {} with no user or group, {} unresolved (no SID, \
         not another domain's), {} skipped (another domain's, no SID), {} not deleted, {} \
         running instances stopped, {} not, {cancelled} BITS jobs cancelled, {failed} \
         not{unlisted}{capped}{listed}",
        swept.folders,
        swept.tasks,
        swept.deleted,
        swept.unreadable,
        swept.file_skipped,
        swept.no_principal,
        swept.unresolved,
        swept.skipped,
        swept.not_deleted,
        swept.instances_stopped,
        swept.instances_not_stopped
    )
}

/// Step 4: every folder from the root, depth first through a stack, at most [`FOLDERS_CAP`];
/// each task read, resolved and, when the pool's, deleted ([`sweep_task`]). Each name is
/// resolved once.
pub fn sweep_tasks<S: Sweeper>(fs: &mut S, pool: &[Sid]) -> Swept {
    let mut swept = Swept::default();
    let mut resolved = BTreeMap::new();
    let names = Names::of(fs, pool);
    let mut stack = match fs.root() {
        Ok(root) => vec![root],
        Err(why) => {
            swept.unlisted += 1;
            swept.note(why);
            return swept;
        }
    };
    while let Some(folder) = stack.pop() {
        if swept.folders >= FOLDERS_CAP {
            swept.capped = true;
            break;
        }
        swept.folders += 1;
        match fs.tasks(&folder) {
            Ok(tasks) => {
                for task in tasks {
                    let at = Walk {
                        pool,
                        names: &names,
                        resolved: &mut resolved,
                        swept: &mut swept,
                    };
                    sweep_task(fs, &folder, task, at);
                }
            }
            Err(why) => {
                swept.unlisted += 1;
                swept.note(why);
            }
        }
        match fs.subfolders(&folder) {
            Ok(subs) => {
                for sub in subs {
                    match sub {
                        Ok(sub) => stack.push(sub),
                        Err(why) => {
                            swept.unlisted += 1;
                            swept.note(why);
                        }
                    }
                }
            }
            Err(why) => {
                swept.unlisted += 1;
                swept.note(why);
            }
        }
    }
    swept
}

/// What one task's sweep works with.
struct Walk<'w> {
    /// The pool's SIDs.
    pool: &'w [Sid],
    /// The pool's names, for a name that resolves to no SID.
    names: &'w Names,
    /// Each name's resolution, once a run.
    resolved: &'w mut BTreeMap<String, Resolved>,
    /// The counts.
    swept: &'w mut Swept,
}

/// What one task's principal or owner makes of it.
enum Found {
    /// The pool's: deleted.
    Pool,
    /// Not the pool's: left.
    NotPool,
    /// Another domain's name that resolves to no SID: skipped.
    Skipped(Failed),
    /// A name that resolves to no SID and may be the pool's: 56.
    Unresolved(Failed),
    /// No user or group: 56.
    NoPrincipal,
    /// The owner couldn't be read, or the task file read instead couldn't be read or trusted,
    /// or names the pool or no one known: 56.
    Unread(Failed),
    /// The Task Scheduler wouldn't give it, and its trusted task file names a principal that
    /// isn't the pool's: skipped.
    FileSkipped,
}

/// One task of `folder`: deleted when its principal is the pool's (a pool SID, or a pool
/// user's name that resolves to no SID) or its owner is a pool SID, counted in `swept` either
/// way. A task the Task Scheduler won't give is held to its owner and its file
/// ([`file_trust::by_file`]).
fn sweep_task<S: Sweeper>(fs: &mut S, folder: &S::Folder, task: Task, at: Walk<'_>) {
    let Walk {
        pool,
        names,
        resolved,
        swept,
    } = at;
    swept.tasks += 1;
    let found = match task.principal {
        Ok(principal) => {
            let found = match principal.name() {
                Some(name) => by_name(fs, pool, names, resolved, name),
                None => Found::NoPrincipal,
            };
            if matches!(found, Found::Pool) {
                found
            } else {
                match pool_owned(fs, folder, &task.name, pool) {
                    Ok(true) => Found::Pool,
                    Ok(false) => found,
                    Err(why) => Found::Unread(why),
                }
            }
        }
        Err(why) => {
            let found = file_trust::by_file(fs, folder, &task.name, pool, names, resolved);
            if matches!(found, Found::Unread(_)) {
                swept.note(why);
            }
            found
        }
    };
    match found {
        Found::Pool => {
            // Its instances first: deleting the task ends none.
            running::stop_instances(fs, folder, &task.name, swept);
            match fs.delete_task(folder, &task.name) {
                Ok(()) => swept.deleted += 1,
                Err(why) => {
                    swept.not_deleted += 1;
                    swept.note(why);
                }
            }
        }
        Found::NotPool => {}
        Found::Skipped(why) => {
            swept.skipped += 1;
            swept.note(why);
        }
        Found::Unresolved(why) => {
            swept.unresolved += 1;
            swept.note(why);
        }
        Found::NoPrincipal => swept.no_principal += 1,
        Found::Unread(why) => {
            swept.unreadable += 1;
            swept.note(why);
        }
        Found::FileSkipped => swept.file_skipped += 1,
    }
}

/// What the principal `name` makes of a task, against `pool`; resolved once a run
/// ([`resolve`]), kept in `resolved`.
fn by_name(
    fs: &mut impl Sweeper,
    pool: &[Sid],
    names: &Names,
    resolved: &mut BTreeMap<String, Resolved>,
    name: &str,
) -> Found {
    let found = resolved
        .entry(name.to_owned())
        .or_insert_with(|| resolve(fs, names, name))
        .clone();
    match found {
        Resolved::Sid(sid) if pool.contains(&sid) => Found::Pool,
        Resolved::Sid(_) => Found::NotPool,
        Resolved::PoolName => Found::Pool,
        Resolved::Skipped(why) => Found::Skipped(why),
        Resolved::Unresolved(why) => Found::Unresolved(why),
    }
}
