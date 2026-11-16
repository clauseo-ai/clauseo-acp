//! The deleted-file limit's history half: an earlier version's file may be read only while the
//! matter has held it ever since.
//!
//! From the version on to the last checkpoint, each checkpoint's tree must hold the path (one
//! tree lookup each, [`clauseo_matters::in_version`]). Where one does not, the step into it is
//! diffed (`Matter::changes_between`): a recorded `Renamed` from the path (exact content only)
//! carries on under the new name; anything else is a gap. A gap closes only at a checkpoint the
//! lawyer took to bring files back, an `undo` or a `restore`, whose tree holds the path again;
//! the path coming back any other way (the agent writing it in a turn, the lawyer or another
//! program making a file of that name, seen at a `before-turn` or `outside-edits` checkpoint)
//! refuses the read for good. A gap still open at the last checkpoint refuses it too. The
//! caller then checks the folder now under the name the walk ends with (`holds_now`), in the same
//! job as the walk's last step, so no checkpoint comes between.
//!
//! The walk looks up at most [`LOOKUPS_PER_JOB`] checkpoints per job on the matter's thread, and
//! reads the timeline afresh in each, so checkpoints taken meanwhile are walked too.

use clauseo_matters::{Matter, in_version};
use clauseo_protocol::{ChangeKind, CheckpointId, CheckpointKind, MatterPath};

use super::LOOKUPS_PER_JOB;
use super::texts::Refusal;
use crate::log_file::Cause;

/// Where the walk is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Walk {
    /// The next timeline index to look at.
    next: usize,
    /// The path as the checkpoint before `next` holds it (the version's path, renamed since).
    path: MatterPath,
    /// The path left the matter and has not come back by an undo or a restore.
    gap: bool,
}

/// What one job of the walk found.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// More checkpoints to look at, in the next job.
    More(Walk),
    /// Held to the last checkpoint, under this path.
    Held(MatterPath),
}

impl Walk {
    /// The walk from `version` (on `matter`'s timeline) for `path`.
    pub(super) fn from(matter: &Matter, version: CheckpointId, path: MatterPath) -> Option<Self> {
        let index = matter
            .timeline()
            .iter()
            .position(|entry| entry.id == version)?;
        Some(Self {
            next: index + 1,
            path,
            gap: false,
        })
    }

    /// Looks at up to [`LOOKUPS_PER_JOB`] more checkpoints.
    pub(super) fn step(mut self, matter: &Matter) -> Result<Step, Refusal> {
        let timeline = matter.timeline();
        for _ in 0..LOOKUPS_PER_JOB {
            let (Some(entry), Some(previous)) = (
                timeline.get(self.next),
                self.next.checked_sub(1).and_then(|at| timeline.get(at)),
            ) else {
                break;
            };
            let held =
                in_version(matter, entry.id, &self.path).map_err(|error| not_read(&error))?;
            if self.gap {
                if held {
                    if !matches!(entry.kind, CheckpointKind::Undo | CheckpointKind::Restore) {
                        return Err(Refusal::Deleted);
                    }
                    self.gap = false;
                }
            } else if !held {
                match renamed_to(matter, previous.id, entry.id, &self.path)? {
                    Some(now) => self.path = now,
                    None => self.gap = true,
                }
            }
            self.next += 1;
        }
        if self.next < timeline.len() {
            Ok(Step::More(self))
        } else if self.gap {
            Err(Refusal::Deleted)
        } else {
            Ok(Step::Held(self.path))
        }
    }
}

/// Where the step from `before` to `after` renamed `path` to, exact content only.
fn renamed_to(
    matter: &Matter,
    before: CheckpointId,
    after: CheckpointId,
    path: &MatterPath,
) -> Result<Option<MatterPath>, Refusal> {
    let changes = matter
        .changes_between(before, after)
        .map_err(|error| not_read(&error))?;
    Ok(changes.into_iter().find_map(|change| {
        if let ChangeKind::Renamed { from } = change.change()
            && from == path
        {
            Some(change.path().clone())
        } else {
            None
        }
    }))
}

fn not_read(error: &clauseo_matters::Error) -> Refusal {
    tracing::warn!(error = %Cause(error), "a version's history did not read");
    Refusal::NotRead
}
