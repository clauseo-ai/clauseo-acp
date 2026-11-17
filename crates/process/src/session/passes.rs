//! The passes of a session's sweep, over a [`Table`] of processes: the OS's (`os/linux/marked.rs`,
//! `os/macos/marked.rs`) or a test's.

use std::collections::BTreeSet;
use std::time::Duration;

use super::SessionSwept;
use crate::Timer;

/// The most listings a sweep acts on; one more listing then says whether any is left.
pub(crate) const MAX_PASSES: u32 = 5;
/// How long a pass waits at a time for the processes it killed to stop running.
pub(crate) const DEATH_POLL: Duration = Duration::from_millis(20);
/// How many of those waits a pass takes at most: 200 ms, so five passes wait 1 s at most.
pub(crate) const DEATH_POLLS: u32 = 10;

/// The processes of one machine as a sweep sees them. Every read is of the process now; none
/// signals anything but [`Table::signal`].
pub(crate) trait Table {
    /// What holds a process still between its last read and its signal (a pidfd on Linux).
    type Pin;

    /// Every pid now, or `None` when the listing fails.
    fn pids(&self) -> Option<Vec<u32>>;

    /// This process's own pid, which is never signalled.
    fn own(&self) -> u32;

    /// The start of process `pid` when it runs (not a zombie), its real and effective uid are
    /// both this process's real uid, and its environment holds `entry` as a whole entry; `None`
    /// when any of that fails or cannot be read.
    fn marked(&self, pid: u32, entry: &[u8]) -> Option<u64>;

    /// Pins process `pid`, so a signal through the pin reaches only the process that held the
    /// pid when it was pinned; `None` when it is gone.
    fn pin(&self, pid: u32) -> Option<Self::Pin>;

    /// Sends SIGKILL to the pinned process; whether the signal went.
    fn signal(&self, pid: u32, pin: &Self::Pin) -> bool;

    /// Whether process `pid` still runs with `start`.
    fn runs(&self, pid: u32, start: u64) -> bool;
}

/// Kills every process `table` lists as carrying `entry`, pass after pass (see
/// `SessionMark::sweep`).
pub(crate) async fn sweep<T: Table>(table: &T, entry: &[u8], timer: &impl Timer) -> SessionSwept {
    let mut ended: BTreeSet<(u32, u64)> = BTreeSet::new();
    for _ in 0..MAX_PASSES {
        let Some(found) = listed(table, entry) else {
            return failed(ended.len());
        };
        if found.is_empty() {
            return SessionSwept::Swept {
                ended: ended.len(),
                left: false,
            };
        }
        let killed: Vec<(u32, u64)> = found
            .into_iter()
            .filter(|&(pid, start)| end(table, pid, start, entry))
            .collect();
        ended.extend(killed.iter().copied());
        for _ in 0..DEATH_POLLS {
            if killed.iter().all(|&(pid, start)| !table.runs(pid, start)) {
                break;
            }
            timer.after(DEATH_POLL).await;
        }
    }
    match listed(table, entry) {
        Some(found) => SessionSwept::Swept {
            ended: ended.len(),
            left: !found.is_empty(),
        },
        None => failed(ended.len()),
    }
}

/// What a sweep says when a listing failed after it had ended `ended` processes.
fn failed(ended: usize) -> SessionSwept {
    if ended == 0 {
        SessionSwept::NotListed
    } else {
        SessionSwept::Swept { ended, left: true }
    }
}

/// The processes carrying `entry` now, with their starts, this process left out; `None` when
/// the listing fails.
fn listed<T: Table>(table: &T, entry: &[u8]) -> Option<Vec<(u32, u64)>> {
    let own = table.own();
    Some(
        table
            .pids()?
            .into_iter()
            .filter(|&pid| pid != own && pid != 0)
            .filter_map(|pid| table.marked(pid, entry).map(|start| (pid, start)))
            .collect(),
    )
}

/// Kills process `pid` if, once pinned, it is still the process listed with `start` and still
/// carries `entry`; whether the signal went.
fn end<T: Table>(table: &T, pid: u32, start: u64, entry: &[u8]) -> bool {
    let Some(pin) = table.pin(pid) else {
        return false;
    };
    // Read again after the pin: a pid reused since the listing has another start, or no entry.
    if table.marked(pid, entry) != Some(start) {
        return false;
    }
    table.signal(pid, &pin)
}
