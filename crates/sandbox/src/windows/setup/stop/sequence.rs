//! The stop's sequence: the number a run reserves from `StopNext` before it observes anything
//! (step 3a), and the completion that writes it as `StopSequence` (step 7). Both run under the
//! set-up lock, and both check the record still holds the pool this run works from, so a stop
//! still running after `--remove` writes nothing.

use std::collections::BTreeSet;

use super::{Refusal, StopExit, Stopper};
use crate::windows::setup::record_values::{STOP_COMPLETED, STOP_NEXT, STOP_SEQUENCE};
use crate::windows::sids::Sid;

/// Step 3a: under the set-up lock, `StopNext` read (1 when absent) and written one higher; the
/// number read is this run's `n`. Nothing is stopped yet, so every failure stops nothing: a
/// lock that stays held, a record that no longer holds `pool`, or a write that fails is
/// [`StopExit::NotReserved`]; a read that fails, or a `StopNext` at its end, is
/// [`StopExit::NoRecord`]. The task runs one instance at a time, so no two reservations overlap.
pub(super) fn reserve(fs: &mut impl Stopper, pool: &[Sid]) -> Result<u64, Refusal> {
    let refuse = |exit: StopExit, why: String| {
        (
            exit,
            format!("nothing was stopped: StopNext wasn't reserved: {why}"),
        )
    };
    match fs.lock() {
        Ok(true) => {}
        Ok(false) => {
            let why = "the set-up lock stayed held".to_owned();
            return Err(refuse(StopExit::NotReserved, why));
        }
        Err(why) => return Err(refuse(StopExit::NotReserved, why)),
    }
    let reserved = reserve_locked(fs, pool);
    fs.unlock();
    reserved.map_err(|(exit, why)| refuse(exit, why))
}

/// [`reserve`]'s steps, with the lock held.
fn reserve_locked(fs: &mut impl Stopper, pool: &[Sid]) -> Result<u64, Refusal> {
    let still = fs.pool().map_err(|why| (StopExit::NoRecord, why))?;
    if !holds(still.map(|pool| pool.users), pool) {
        let why = "the record is gone, or its pool changed".to_owned();
        return Err((StopExit::NotReserved, why));
    }
    let n = fs
        .read_stop_value(STOP_NEXT)
        .map_err(|why| (StopExit::NoRecord, why))?
        .unwrap_or(1);
    let Some(next) = n.checked_add(1) else {
        return Err((StopExit::NoRecord, "StopNext is at its end".to_owned()));
    };
    fs.write_stop_value(STOP_NEXT, next)
        .map_err(|why| (StopExit::NotReserved, why))?;
    Ok(n)
}

/// Step 7: under the set-up lock, once everything before has passed, `StopCompleted` (`at`, the
/// last clean observation) written first, then `StopSequence` as `n`, the number [`reserve`]
/// kept. Nothing is written when the record no longer holds `pool` (the key gone, or the pool
/// changed), or when `StopSequence` is already past `n`. The lock's wait may come a second
/// time here only when a set-up took the lock after the marks; a run the task's limit ends
/// meanwhile writes nothing.
pub(super) fn completed(
    fs: &mut impl Stopper,
    at: u64,
    n: u64,
    pool: &[Sid],
) -> Result<(), String> {
    let written = match fs.lock() {
        Ok(true) => {
            let written = completed_locked(fs, at, n, pool);
            fs.unlock();
            written
        }
        Ok(false) => Err("the set-up lock stayed held".to_owned()),
        Err(why) => Err(why),
    };
    written.map_err(|why| format!("StopCompleted wasn't written: {why}"))
}

/// [`completed`]'s steps, with the lock held.
fn completed_locked(fs: &mut impl Stopper, at: u64, n: u64, pool: &[Sid]) -> Result<(), String> {
    let still = fs.pool()?;
    if !holds(still.map(|pool| pool.users), pool) {
        return Err("the record is gone, or its pool changed since the run began".to_owned());
    }
    if let Some(seen) = fs.read_stop_value(STOP_SEQUENCE)?
        && seen > n
    {
        return Err(format!(
            "StopSequence is already {seen}, past this run's {n}"
        ));
    }
    fs.write_stop_value(STOP_COMPLETED, at)?;
    fs.write_stop_value(STOP_SEQUENCE, n)
        .map_err(|why| format!("StopSequence wasn't written after it: {why}"))
}

/// Whether the record's pool as read now (`None`: no record) is the set `pool` was.
fn holds(now: Option<Vec<Sid>>, pool: &[Sid]) -> bool {
    let Some(now) = now else {
        return false;
    };
    let now: BTreeSet<&Sid> = now.iter().collect();
    now == pool.iter().collect::<BTreeSet<&Sid>>()
}
