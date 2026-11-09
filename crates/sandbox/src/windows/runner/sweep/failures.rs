//! A sweep's failures, round by round: the first act's first ends the sweep; past it, each is
//! logged once a sweep and counted for its round, escapees (survivors among them) apart.

use std::collections::BTreeSet;

use super::{IMAGES_CAP, Point, SweepError};

/// A round's failures: the first act's first ends the sweep; past it, each is logged once a
/// sweep and counted for the round, escapees apart.
pub(super) struct Failures {
    pub(super) point: Point,
    pub(super) logged: BTreeSet<u32>,
    pub(super) count: usize,
    /// How many of this round's couldn't be opened or ended.
    pub(super) unkillable: usize,
    /// Their image names, each once, at most [`IMAGES_CAP`].
    pub(super) images: Vec<String>,
    /// The pids counted this round: each is counted once a round.
    pub(super) this_round: BTreeSet<u32>,
}

impl Failures {
    /// The verdict of a sweep cut short (out of rounds or time): [`SweepError::Left`] when the
    /// last round counted an escapee (a survivor among them), so the slot is quarantined; else
    /// `otherwise`.
    pub(super) fn verdict(&mut self, otherwise: SweepError) -> SweepError {
        if self.unkillable == 0 {
            return otherwise;
        }
        self.left()
    }

    /// The round's failures as [`SweepError::Left`].
    pub(super) fn left(&mut self) -> SweepError {
        SweepError::Left {
            count: self.count,
            unkillable: self.unkillable,
            images: std::mem::take(&mut self.images),
        }
    }

    /// Starts a round's count.
    pub(super) fn next_round(&mut self) {
        self.count = 0;
        self.unkillable = 0;
        self.images.clear();
        self.this_round.clear();
    }

    pub(super) fn add(
        &mut self,
        error: SweepError,
        log: &mut dyn FnMut(&str),
    ) -> Result<(), SweepError> {
        if self.point == Point::Start {
            return Err(error);
        }
        let pid = pid_of(&error);
        if pid != 0 && !self.this_round.insert(pid) {
            return Ok(());
        }
        self.count += 1;
        if let Some(escapees) = error.escapees() {
            self.unkillable += escapees.count;
            for image in escapees.images {
                if self.images.len() < IMAGES_CAP && !self.images.contains(&image) {
                    self.images.push(image);
                }
            }
        }
        if self.logged.insert(pid) {
            log(&format!(
                "clauseo-sandbox-run: sweep ({}): {error}",
                self.point
            ));
        }
        Ok(())
    }
}

fn pid_of(error: &SweepError) -> u32 {
    match error {
        SweepError::Open { pid, .. }
        | SweepError::Kill { pid, .. }
        | SweepError::Survived { pid, .. }
        | SweepError::Started { pid, .. } => *pid,
        SweepError::List(_)
        | SweepError::JobList(_)
        | SweepError::StillRunning
        | SweepError::Deadline
        | SweepError::Left { .. }
        | SweepError::OwnStart { .. } => 0,
    }
}
