//! A sweep that ended 56 (`SweepExit::TasksLeft`) for the number a marker awaits: it already
//! observed after the marker, and found a task it can't read or delete, so a wait for the next
//! one would end the same way, 15 s later. The launch and F4 refuse at once instead, with the
//! fresh sweep still requested: once an administrator deletes the task, that sweep (or a later
//! one) ends 0 and the next launch goes. The sweep records its last exit and number
//! (`SweepLastExit`, `SweepLastSequence`); any other exit, or none recorded, is the wait as
//! before.

use super::Markers;
use crate::windows::setup::sweep::SweepExit;

/// Why a launch's check refused ([`super::await_sweeps`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unswept {
    /// The sweep a marker awaits didn't complete within [`super::SWEEP_WAIT`], or the markers
    /// or the sequences couldn't be read, or a marker couldn't be moved on: `UncleanSession`,
    /// which a later launch checks again.
    Waiting(String),
    /// The last sweep ended 56 having served the number a marker awaits:
    /// `Blocked::LeftoverTask`, which only an administrator can lift.
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
pub(super) const LEFTOVER: &str = "an earlier session's sandbox sweep ended 56: a scheduled task \
     it can't remove is left (another sweep was requested)";

/// Whether the last sweep recorded ended 56 having served `wanted` or a later number, so no
/// sweep since has passed (a 0 run writes its number as `SweepSequence`, which would have met
/// `wanted`).
pub(super) fn left_at(fs: &mut impl Markers, wanted: u64) -> bool {
    let tasks_left = u32::from(SweepExit::TasksLeft.as_code());
    fs.last_sweep()
        .is_some_and(|(exit, sequence)| exit == tasks_left && sequence >= wanted)
}
