//! A pool task's sequence, shared by `--stop` (`StopNext`, `StopSequence`, `StopCompleted`) and
//! `--sweep-tasks` (`SweepNext`, `SweepSequence`, `SweepCompleted`): the number a run reserves
//! from its `…Next` before it observes anything, and the completion that writes it as its
//! `…Sequence`. Both run under the set-up lock, and both check the record still holds the pool
//! this run works from, so a run still going after `--remove` writes nothing.
//!
//! A marker that waits for a run (`runner::marker`) reads `…Next` as the lawyer, without the
//! lock (only an administrator's token can open it), once its session can launch nothing more,
//! and is clean once `…Sequence` reaches what it read. That holds without the lock: `…Next` is
//! one `REG_QWORD` that only goes up, and a run writes `n + 1` before it observes. A marker that
//! read `M` read it before the run that takes `M` wrote `M + 1`, so that run observes after the
//! marker's read. `--remove` deletes `…Sequence` before `…Next` (`SETUP_VALUES`), so a read
//! during a removal can only keep a marker longer.

use std::collections::BTreeSet;

use super::{Failed, Record};
use crate::windows::setup::record_values::{
    STOP_COMPLETED, STOP_NEXT, STOP_SEQUENCE, SWEEP_COMPLETED, SWEEP_NEXT, SWEEP_SEQUENCE,
};
use crate::windows::sids::Sid;

/// One task's three record values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Values {
    /// `…Next` (`REG_QWORD`, 1 when absent): the number the next run reserves.
    pub next: &'static str,
    /// `…Sequence` (`REG_QWORD`): the number the last completed run reserved.
    pub sequence: &'static str,
    /// `…Completed` (`REG_QWORD`, a `FILETIME`): when its last clean observation began.
    pub completed: &'static str,
}

/// The stop's values.
pub const STOP: Values = Values {
    next: STOP_NEXT,
    sequence: STOP_SEQUENCE,
    completed: STOP_COMPLETED,
};

/// The sweep's values.
pub const SWEEP: Values = Values {
    next: SWEEP_NEXT,
    sequence: SWEEP_SEQUENCE,
    completed: SWEEP_COMPLETED,
};

/// Why a reservation failed; nothing was observed yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotReserved {
    /// The set-up lock stayed held, or couldn't be taken.
    Lock(Failed),
    /// The record is gone or holds another pool, or the write failed.
    Changed(Failed),
    /// `…Next` couldn't be read, or is at its end.
    Unread(Failed),
}

/// Under the set-up lock, `values.next` read (1 when absent) and written one higher; the number
/// read is this run's `n`. Each task runs one instance at a time, so no two of its reservations
/// overlap.
pub fn reserve(fs: &mut impl Record, values: Values, pool: &[Sid]) -> Result<u64, NotReserved> {
    match fs.lock() {
        Ok(true) => {}
        Ok(false) => {
            return Err(NotReserved::Lock("the set-up lock stayed held".to_owned()));
        }
        Err(why) => return Err(NotReserved::Lock(why)),
    }
    let reserved = reserve_locked(fs, values, pool);
    fs.unlock();
    reserved
}

/// [`reserve`]'s steps, with the lock held.
fn reserve_locked(fs: &mut impl Record, values: Values, pool: &[Sid]) -> Result<u64, NotReserved> {
    let still = fs.pool().map_err(NotReserved::Unread)?;
    if !holds(still.map(|pool| pool.users), pool) {
        let why = "the record is gone, or its pool changed".to_owned();
        return Err(NotReserved::Changed(why));
    }
    let n = fs
        .read_value(values.next)
        .map_err(NotReserved::Unread)?
        .unwrap_or(1);
    let Some(next) = n.checked_add(1) else {
        return Err(NotReserved::Unread(format!(
            "{} is at its end",
            values.next
        )));
    };
    fs.write_value(values.next, next)
        .map_err(NotReserved::Changed)?;
    Ok(n)
}

/// Under the set-up lock, once everything before has passed, `values.completed` (`at`, the
/// last clean observation) written first, then `values.sequence` as `n`, the number [`reserve`]
/// kept. Nothing is written when the record no longer holds `pool` (the key gone, or the pool
/// changed), or when the sequence is already past `n`.
pub fn completed(
    fs: &mut impl Record,
    values: Values,
    at: u64,
    n: u64,
    pool: &[Sid],
) -> Result<(), Failed> {
    let written = match fs.lock() {
        Ok(true) => {
            let written = completed_locked(fs, values, at, n, pool);
            fs.unlock();
            written
        }
        Ok(false) => Err("the set-up lock stayed held".to_owned()),
        Err(why) => Err(why),
    };
    written.map_err(|why| format!("{} wasn't written: {why}", values.completed))
}

/// [`completed`]'s steps, with the lock held.
fn completed_locked(
    fs: &mut impl Record,
    values: Values,
    at: u64,
    n: u64,
    pool: &[Sid],
) -> Result<(), Failed> {
    let still = fs.pool()?;
    if !holds(still.map(|pool| pool.users), pool) {
        return Err("the record is gone, or its pool changed since the run began".to_owned());
    }
    if let Some(seen) = fs.read_value(values.sequence)?
        && seen > n
    {
        let name = values.sequence;
        return Err(format!("{name} is already {seen}, past this run's {n}"));
    }
    fs.write_value(values.completed, at)?;
    fs.write_value(values.sequence, n)
        .map_err(|why| format!("{} wasn't written after it: {why}", values.sequence))
}

/// Whether the record's pool as read now (`None`: no record) is the set `pool` was.
pub fn holds(now: Option<Vec<Sid>>, pool: &[Sid]) -> bool {
    let Some(now) = now else {
        return false;
    };
    let now: BTreeSet<&Sid> = now.iter().collect();
    now == pool.iter().collect::<BTreeSet<&Sid>>()
}
