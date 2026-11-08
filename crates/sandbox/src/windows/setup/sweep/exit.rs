//! The sweep's exit codes, and where it records its last run's (`SweepLastExit`,
//! `SweepLastSequence`) for the launches that wait on it (`runner::unclean`).

use super::super::pool_steps::Failed;

/// The sweep's exit codes. They start at 53, after [`StopExit`]'s 50 and the WSL deny task's 51
/// and 52 (`WslDenyExit`), so the one exe never gives one number two meanings; codes are
/// appended from here on, never renumbered.
///
/// [`StopExit`]: super::super::stop::StopExit
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
    /// tasks or subfolders couldn't be listed, or there were more than
    /// [`FOLDERS_CAP`](super::FOLDERS_CAP) folders; or a task file or the Task Scheduler's cache
    /// holds a task the listing didn't give, and its file doesn't show it isn't the pool's, or
    /// either couldn't be listed (`unlisted`).
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

/// Where the sweep records its last run, in the record's key next to its numbers, but apart
/// from [`Record`](super::Record): written without the set-up lock, by the one instance the
/// task runs at a time. Read by the lawyer (the key grants Users read).
pub trait LastRun {
    /// Sets `SweepLastExit` (`REG_DWORD`) to `exit`, never making the key.
    fn write_last_exit(&mut self, exit: u32) -> Result<(), Failed>;
    /// Sets `SweepLastSequence` (`REG_QWORD`) to `sequence`, never making the key.
    fn write_last_sequence(&mut self, sequence: u64) -> Result<(), Failed>;
}

/// Records the run's `exit`, then the number `n` it reserved, if any: the exit first, so a
/// reader between the two writes pairs a new exit with an older number, which can only make it
/// wait (`runner::unclean`), never pairs an old 56 with this run's number. A run that reserved
/// none leaves `SweepLastSequence` as it was.
pub(super) fn record_last(
    fs: &mut impl LastRun,
    exit: SweepExit,
    n: Option<u64>,
) -> Result<(), Failed> {
    fs.write_last_exit(u32::from(exit.as_code()))
        .map_err(|why| format!("SweepLastExit wasn't written: {why}"))?;
    match n {
        Some(n) => fs
            .write_last_sequence(n)
            .map_err(|why| format!("SweepLastSequence wasn't written: {why}")),
        None => Ok(()),
    }
}
