//! A sweep that ended 56 (`SweepExit::TasksLeft`) for the number a marker awaits: it observed
//! after the marker and found a task it can't read or delete. The sweep records its last exit
//! and number (`SweepLastExit`, `SweepLastSequence`), and the launch and F4 read them.
//!
//! When the last run had ended 56 at or past the number awaited (`seen`), the wait asks for a
//! fresh run, unless one is already under way (`SweepNext` past `seen + 1`: a number reserved
//! whose exit isn't recorded yet; a wait that asked for none asks once it ends, so a run that
//! died without recording doesn't hold every request back). It then polls as before, up to
//! [`SWEEP_WAIT`], and also reads the last run each time: `SweepSequence` reaching the number is
//! clean, as before; a newer run's 56 ends it as a leftover task. At its end a 56 still
//! standing is one too. An administrator who deleted the task just before the launch is thus
//! seen on that launch: its fresh run ends 0. Any other last exit, or none recorded, is the
//! wait as before.
//!
//! Each read takes the last run first, then the sequences: a run that ends 0 writes
//! `SweepSequence` before `SweepLastExit`, so an old 56 read before that run's exit is
//! overruled by its sequence, read after.

use super::{Markers, SWEEP_POLL, SWEEP_WAIT, Sequences};
use crate::windows::setup::record_values::SWEEP_NEXT;
use crate::windows::setup::sweep::SweepExit;

/// Why a launch's check refused ([`super::await_sweeps`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unswept {
    /// The sweep a marker awaits didn't complete within [`SWEEP_WAIT`], or the markers or the
    /// sequences couldn't be read, or a marker couldn't be moved on: `UncleanSession`, which a
    /// later launch checks again.
    Waiting(String),
    /// The last sweep had ended 56 having served the number a marker awaits, and a fresh run
    /// ended 56 too, or none did within the wait: `Blocked::LeftoverTask`, which only an
    /// administrator can lift.
    LeftoverTask(String),
}

impl Unswept {
    /// What it says, for the log.
    pub fn why(&self) -> &str {
        match self {
            Unswept::Waiting(why) | Unswept::LeftoverTask(why) => why,
        }
    }
}

/// What a refusal for a leftover task says.
pub(super) const LEFTOVER: &str = "the sandbox's sweep after an earlier session ended 56 again: \
     a scheduled task it can't remove is left";

/// What a wait for the sweep `wanted` came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Waited {
    /// `SweepSequence` reached it, as these sequences show.
    Swept(Sequences),
    /// A 56 at or past it, as the module says.
    Leftover,
    /// Neither, within [`SWEEP_WAIT`].
    Short,
}

/// One read: the last run, then the sequences.
enum Read {
    Swept(Sequences),
    /// The last run ended 56 at this number, at or past `wanted`, and no 0 run has met it.
    Left(u64),
    Neither,
}

fn read(fs: &mut impl Markers, wanted: u64) -> Result<Read, String> {
    let tasks_left = u32::from(SweepExit::TasksLeft.as_code());
    let last = fs.last_sweep();
    let done = fs.sequences()?;
    Ok(match last {
        _ if done.sweep >= wanted => Read::Swept(done),
        Some((exit, sequence)) if exit == tasks_left && sequence >= wanted => Read::Left(sequence),
        _ => Read::Neither,
    })
}

/// Waits for the sweep `wanted`, which `SweepSequence` doesn't meet yet, as the module says;
/// `requested`: a re-mark asked for one already. `Err` when the sequences can't be read.
pub(super) fn wait_for(
    fs: &mut impl Markers,
    wanted: u64,
    requested: bool,
) -> Result<Waited, String> {
    let seen = match read(fs, wanted)? {
        Read::Swept(done) => return Ok(Waited::Swept(done)),
        Read::Left(seen) => Some(seen),
        Read::Neither => None,
    };
    // Under way: a number past `seen` reserved, its exit not recorded yet.
    let under_way = seen.is_some_and(|seen| {
        fs.next_number(SWEEP_NEXT)
            .is_ok_and(|next| next > seen.saturating_add(1))
    });
    let asked = requested || under_way;
    if !asked {
        drop(fs.request_sweep());
    }
    let polls = SWEEP_WAIT.as_millis() / SWEEP_POLL.as_millis().max(1);
    for _ in 0..polls {
        fs.pause(SWEEP_POLL);
        match seen {
            // As before: only `SweepSequence` is read.
            None => {
                let done = fs.sequences()?;
                if done.sweep >= wanted {
                    return Ok(Waited::Swept(done));
                }
            }
            Some(seen) => match read(fs, wanted)? {
                Read::Swept(done) => return Ok(Waited::Swept(done)),
                Read::Left(newer) if newer > seen => return Ok(Waited::Leftover),
                Read::Left(_) | Read::Neither => {}
            },
        }
    }
    if under_way && !requested {
        drop(fs.request_sweep());
    }
    if seen.is_none() {
        return Ok(Waited::Short);
    }
    // Read again before refusing: a 0 run may have completed since the last poll.
    Ok(match read(fs, wanted)? {
        Read::Swept(done) => Waited::Swept(done),
        Read::Left(_) => Waited::Leftover,
        Read::Neither => Waited::Short,
    })
}
