//! The per-turn caps: [`MAX_CALLS_PER_TURN`] calls and [`MAX_DOCX_RUNS_PER_TURN`] stetdoc runs
//! per turn, and the same again for each stretch between turns, as the case-law relay counts.

use super::{MAX_CALLS_PER_TURN, MAX_DOCX_RUNS_PER_TURN};

/// The counts of the current turn, or of the stretch since the last one ended.
#[derive(Debug, Default)]
pub(super) struct Caps {
    open: bool,
    calls: u32,
    docx_runs: u32,
}

impl Caps {
    /// A turn starts: its counts start at 0.
    pub(super) fn start_turn(&mut self) {
        *self = Self {
            open: true,
            ..Self::default()
        };
    }

    /// The turn ended: what follows counts against a fresh between-turns bucket.
    pub(super) fn end_turn(&mut self) {
        if self.open {
            *self = Self::default();
        }
    }

    /// Counts one call; `false` once [`MAX_CALLS_PER_TURN`] were made.
    pub(super) fn take_call(&mut self) -> bool {
        take(&mut self.calls, MAX_CALLS_PER_TURN)
    }

    /// Whether a stetdoc run may still start.
    pub(super) const fn docx_run_left(&self) -> bool {
        self.docx_runs < MAX_DOCX_RUNS_PER_TURN
    }

    /// The stetdoc runs counted so far (tests).
    #[cfg(test)]
    pub(super) const fn docx_runs(&self) -> u32 {
        self.docx_runs
    }

    /// Counts one stetdoc run; `false` once [`MAX_DOCX_RUNS_PER_TURN`] were started.
    pub(super) fn take_docx_run(&mut self) -> bool {
        take(&mut self.docx_runs, MAX_DOCX_RUNS_PER_TURN)
    }
}

fn take(count: &mut u32, cap: u32) -> bool {
    if *count >= cap {
        return false;
    }
    *count += 1;
    true
}
