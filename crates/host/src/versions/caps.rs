//! The per-turn caps: [`MAX_CALLS_PER_TURN`] calls and [`MAX_DOCX_RUNS_PER_TURN`] stetdoc runs
//! per turn, and the same again for each stretch between turns, as the case-law relay counts.
//! A stetdoc run is reserved when a read finds it needs one ([`Caps::reserve_docx_run`]) and
//! given back when it turns out not to run ([`Caps::give_back`]), to the bucket it came from
//! only.

use super::{MAX_CALLS_PER_TURN, MAX_DOCX_RUNS_PER_TURN};

/// The counts of the current turn, or of the stretch since the last one ended.
#[derive(Debug, Default)]
pub(super) struct Caps {
    open: bool,
    calls: u32,
    docx_runs: u32,
    /// Bumped whenever a bucket starts, so a give-back reaches only the bucket it was taken from.
    epoch: u64,
}

/// One stetdoc run taken from a bucket, until it runs or is given back.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Reserved {
    epoch: u64,
}

impl Caps {
    /// A turn starts: its counts start at 0.
    pub(super) fn start_turn(&mut self) {
        *self = Self {
            open: true,
            epoch: self.epoch.wrapping_add(1),
            ..Self::default()
        };
    }

    /// The turn ended: what follows counts against a fresh between-turns bucket.
    pub(super) fn end_turn(&mut self) {
        if self.open {
            *self = Self {
                epoch: self.epoch.wrapping_add(1),
                ..Self::default()
            };
        }
    }

    /// Counts one call; `false` once [`MAX_CALLS_PER_TURN`] were made.
    pub(super) fn take_call(&mut self) -> bool {
        take(&mut self.calls, MAX_CALLS_PER_TURN)
    }

    /// Takes one stetdoc run; `None` once [`MAX_DOCX_RUNS_PER_TURN`] were taken.
    pub(super) fn reserve_docx_run(&mut self) -> Option<Reserved> {
        take(&mut self.docx_runs, MAX_DOCX_RUNS_PER_TURN).then_some(Reserved { epoch: self.epoch })
    }

    /// Gives back a run that did not run, if its bucket is still the current one.
    pub(super) fn give_back(&mut self, reserved: &Reserved) {
        if reserved.epoch == self.epoch {
            self.docx_runs = self.docx_runs.saturating_sub(1);
        }
    }

    /// The stetdoc runs counted so far (tests).
    #[cfg(test)]
    pub(super) const fn docx_runs(&self) -> u32 {
        self.docx_runs
    }
}

fn take(count: &mut u32, cap: u32) -> bool {
    if *count >= cap {
        return false;
    }
    *count += 1;
    true
}
