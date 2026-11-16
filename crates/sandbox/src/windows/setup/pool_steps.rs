//! The steps the SYSTEM tasks that act on the pool share: `--stop` (`stop.rs`) and
//! `--sweep-tasks` (`sweep.rs`). Each mode maps what these return to its own exit codes.
//!
//! - [`gate`]: refuses unless this process is LOCAL SYSTEM and runs from the admin copy's path,
//!   then reads the pool from the machine's record (`PoolSids`, `OwnerSid`) and holds every SID
//!   to the pool-user check (`processes::check_pool`). No record, or an empty pool, is nothing
//!   to act on.
//! - [`cancel_pool_jobs`]: cancels every BITS job whose owner is exactly one of the pool's SIDs,
//!   all users' jobs listed, at most [`JOBS_CAP`] of them.
//! - [`sequence`]: the number a run reserves before it observes, and the completion that
//!   writes it, through [`Record`].
//!
//! None makes a Windows call: [`Gate`], [`Bits`] and [`Record`] carry every effect,
//! implemented by `windows_gate.rs`, `windows_bits.rs` and `windows_record.rs` and faked by
//! each mode's tests.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::windows::refresh::same_path;
use crate::windows::sids::Sid;

pub mod sequence;
#[cfg(windows)]
pub(crate) mod windows_bits;
#[cfg(windows)]
pub(crate) mod windows_gate;
#[cfg(windows)]
pub(crate) mod windows_record;

/// What the record gives a pool task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pool {
    /// `OwnerSid`: never acted on, whatever `PoolSids` says.
    pub owner: Sid,
    /// `PoolSids`.
    pub users: Vec<Sid>,
}

/// A failed effect: what failed, as a line for the log.
pub type Failed = String;

/// The most BITS jobs a pool task reads ([`Bits::bits_jobs`]). A list that reaches it may have
/// been cut, so jobs of the pool's may be left past it: the run doesn't end 0.
pub(crate) const JOBS_CAP: usize = 10_000;

/// The gate's effects. The real one reads the record and the admin copy's path itself, so no
/// method takes a SID or a path the caller chose, but the pool the record gave.
pub trait Gate {
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
}

/// The record's effects a pool task's sequence needs ([`sequence`]): the set-up lock, the
/// record's `REG_QWORD` values, and the wall clock.
pub trait Record: Gate {
    /// Waits a bounded time for the set-up lock and keeps it until [`Record::unlock`]:
    /// `Ok(false)` when another still holds it.
    fn lock(&mut self) -> Result<bool, Failed>;
    /// Gives the set-up lock back, if held.
    fn unlock(&mut self);
    /// The record's `REG_QWORD` value `name` (`StopNext`, `SweepSequence`, …): `None` when
    /// it, or the key, is missing. Called under the set-up lock.
    fn read_value(&mut self, name: &'static str) -> Result<Option<u64>, Failed>;
    /// Sets the record's `REG_QWORD` value `name` in the key as it is, never making the key.
    /// Called under the set-up lock.
    fn write_value(&mut self, name: &'static str, value: u64) -> Result<(), Failed>;
    /// The wall clock, as a `FILETIME` (100 ns since 1601, UTC).
    fn now(&mut self) -> u64;
}

/// Why the gate refused; each mode gives each its own exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// Not LOCAL SYSTEM, or not run from the admin copy's path.
    NotAllowed,
    /// The record can't be read, or its `PoolSids` or `OwnerSid` is malformed.
    NoRecord,
    /// A SID `PoolSids` lists failed the pool-user check.
    NotPool,
}

/// The gate as the module says: the pool to act on, `None` when there is none (no record, or an
/// empty pool), or why it refused, with the line for the log. Nothing is acted on before it
/// returns a pool.
pub fn gate(fs: &mut impl Gate) -> Result<Option<Pool>, (Refused, Failed)> {
    let refuse = |why: String| (Refused::NotAllowed, why);
    if !fs.is_system().map_err(refuse)? {
        return Err(refuse("it runs only as LOCAL SYSTEM".to_owned()));
    }
    let exe = fs.current_exe().map_err(refuse)?;
    let admin = fs.admin_copy().map_err(refuse)?;
    if !same_path(&exe, &admin) {
        return Err(refuse("it runs only from the admin copy".to_owned()));
    }
    let pool = match fs.pool().map_err(|why| (Refused::NoRecord, why))? {
        Some(pool) if !pool.users.is_empty() => pool,
        _ => return Ok(None),
    };
    fs.check_pool(&pool)
        .map_err(|why| (Refused::NotPool, why))?;
    Ok(Some(pool))
}

/// The BITS effects.
pub trait Bits {
    /// Every BITS job on this machine, all users', each as its owner: `None` when it couldn't
    /// be read; at most [`JOBS_CAP`]. The list replaces the last one, for [`Bits::cancel_job`].
    fn bits_jobs(&mut self) -> Result<Vec<Option<Sid>>, Failed>;
    /// Cancels the job at `at` in the last list, which removes it.
    fn cancel_job(&mut self, at: usize) -> Result<(), Failed>;
}

/// What [`cancel_pool_jobs`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Jobs {
    /// The pool users that owned a job, cancelled or not.
    pub owners: BTreeSet<Sid>,
    /// The pool users' jobs cancelled.
    pub cancelled: usize,
    /// Jobs not cancelled: a pool user's whose cancel failed, or one whose owner couldn't be
    /// read.
    pub failed: usize,
    /// The list reached [`JOBS_CAP`], so jobs past it may not have been read.
    pub capped: bool,
}

impl Jobs {
    /// Whether a job of the pool's may be left: one not cancelled, an owner not read, or a list
    /// that was cut.
    pub fn left(&self) -> bool {
        self.failed > 0 || self.capped
    }
}

/// Cancels every BITS job whose owner is exactly one of `pool`'s SIDs; another user's is left
/// alone. A list that can't be read is the error; a cancel that fails, or an owner that can't be
/// read, is counted; a list of [`JOBS_CAP`] jobs is marked `capped` once the pool's jobs in it
/// are cancelled.
pub fn cancel_pool_jobs(fs: &mut impl Bits, pool: &[Sid]) -> Result<Jobs, Failed> {
    let owners = fs
        .bits_jobs()
        .map_err(|why| format!("the BITS jobs couldn't be listed: {why}"))?;
    let mut jobs = Jobs::default();
    for (at, owner) in owners.iter().enumerate() {
        match owner {
            Some(owner) if pool.contains(owner) => {
                jobs.owners.insert(owner.clone());
                match fs.cancel_job(at) {
                    Ok(()) => jobs.cancelled += 1,
                    Err(_why) => jobs.failed += 1,
                }
            }
            Some(_) => {}
            None => jobs.failed += 1,
        }
    }
    jobs.capped = owners.len() >= JOBS_CAP;
    Ok(jobs)
}
