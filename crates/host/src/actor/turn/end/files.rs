//! The after-turn checkpoint's part of a turn's close: the matter's `CheckpointRecorded`, the
//! turn's `FilesChanged`, and the notice for files gone with no copy.
//!
//! A file the before-turn checkpoint skipped (cloud-only, or over the size cap) has no version,
//! so its deletion is no change, and no undo brings it back. The checkpoint lists such files as
//! `gone` (`clauseo_matters`' `gone.rs`); the close logs them in `FilesChanged.gone`, which
//! Versions shows on the step, and as a `Notice` (`NoticeKind::FilesGoneNoCopy`) right after it,
//! which the conversation shows. Both come from the same list, so they cannot disagree.
//!
//! Such a file still there with another size or modification time was changed with no copy of
//! how it was before (`Checkpoint::changed_no_copy`): the close tells it in a notice of its own
//! (`NoticeKind::FilesChangedNoCopy`), in the same place. A file only downloaded is not one.

use clauseo_matters::CheckpointOutcome;
use clauseo_protocol::{
    CheckpointKind, CheckpointRecorded, FileChange, FilesChanged, MatterId, Notice, NoticeKind,
    NoticeLevel, SkippedFile, ThreadId,
};

use super::super::super::Actor;
use super::super::close::TurnClose;
use super::super::known;
use crate::error::Error;
use crate::matter_worker::TurnCheckpoint;
use crate::pipeline::MatterEvent;

/// How many gone or changed files a notice names; the rest are counted.
const NAMED: usize = 5;

/// How many of `total` files a notice names: at most [`NAMED`], but all of them when only one
/// would be left to count, so a count is never 1 (the app has no singular line for it).
fn named(total: usize) -> usize {
    if total == NAMED + 1 {
        total
    } else {
        total.min(NAMED)
    }
}

/// The after-turn checkpoint's part of a close: matter `CheckpointRecorded { AfterTurn }` when
/// R8 created it, `FilesChanged` when anything changed, was skipped or is gone with no copy, and
/// `checkpoint_after`.
/// A reused id the matter's log does not hold is recorded first, with the changes the lost
/// commit held (the turn's before-turn checkpoint to that id); no tool-call paths are known
/// for them, so each is flagged as possibly holding the lawyer's edits. The start-up repair's
/// own checkpoint logs those changes even when the id is already recorded.
pub(in super::super) fn after_events(
    actor: &Actor,
    close: &mut TurnClose,
    matter: MatterId,
    thread: ThreadId,
    done: TurnCheckpoint,
) -> Result<(), Error> {
    let turn = close.turn;
    let unrecorded = known::unrecorded(actor, matter, &done)?;
    let TurnCheckpoint { checkpoint, reused } = done;
    let mut files = checkpoint.changes;
    let id = checkpoint.outcome.id();
    if let CheckpointOutcome::Created(id) = checkpoint.outcome {
        let recorded =
            CheckpointRecorded::new(id, CheckpointKind::AfterTurn, Some(thread), Some(turn))
                .map_err(|error| Error::Internal(error.to_string()))?;
        close.push_matter(matter, MatterEvent::checkpoint(recorded, checkpoint.commit));
    }
    let own = reused.as_ref().is_some_and(|reused| reused.own);
    let lost = unrecorded.is_some() || own;
    if let Some(recorded) = unrecorded {
        close.push_matter(matter, recorded);
    }
    if lost {
        let changes = reused.map(|reused| reused.changes).unwrap_or_default();
        files.extend(changes.into_iter().map(FileChange::with_lawyer_edits));
    }
    close.changed_no_copy = checkpoint.changed_no_copy;
    let gone = checkpoint.gone;
    if !files.is_empty() || !checkpoint.skipped.is_empty() || !gone.is_empty() {
        close.files = Some(FilesChanged {
            turn,
            files,
            skipped: checkpoint.skipped,
            gone,
        });
    }
    close.checkpoint_after = Some(id);
    Ok(())
}

/// The notice for the files a turn's `FilesChanged` lists as gone, `None` when there are none:
/// at most [`NAMED`] named (sorted by path bytes, as listed; all six when there are six), the
/// rest counted. Its text, for a
/// view that does not know the kind, is English.
pub(in super::super) fn gone_notice(gone: &[SkippedFile]) -> Option<Notice> {
    let first = gone.first()?;
    let text = if gone.len() == 1 {
        format!(
            "{} was deleted during this answer. Clauseo had no copy of it to bring back.",
            first.path
        )
    } else {
        format!(
            "{} files were deleted during this answer that Clauseo had no copy of, so they can't \
             be brought back.",
            gone.len()
        )
    };
    let shown = named(gone.len());
    let more = u32::try_from(gone.len().saturating_sub(shown)).unwrap_or(u32::MAX);
    let files = gone.iter().take(shown).cloned().collect();
    Some(Notice {
        level: NoticeLevel::Warning,
        text,
        kind: Some(NoticeKind::FilesGoneNoCopy { files, more }),
    })
}

/// The notice for the files the before-turn checkpoint kept no copy of that the turn changed in
/// place (`Checkpoint::changed_no_copy`), `None` when there are none: at most [`NAMED`] named
/// (sorted by path bytes), the rest counted. Its text, for a view that does not know the kind,
/// is English.
pub(in super::super) fn changed_notice(changed: &[SkippedFile]) -> Option<Notice> {
    let first = changed.first()?;
    let text = if changed.len() == 1 {
        format!(
            "{} was changed during this answer. Clauseo had no copy of how it was before, so Undo \
             can't bring the earlier version back.",
            first.path
        )
    } else {
        format!(
            "{} files Clauseo had no earlier copy of were changed during this answer, so Undo \
             can't bring the earlier versions back.",
            changed.len()
        )
    };
    let shown = named(changed.len());
    let more = u32::try_from(changed.len().saturating_sub(shown)).unwrap_or(u32::MAX);
    let files = changed.iter().take(shown).cloned().collect();
    Some(Notice {
        level: NoticeLevel::Warning,
        text,
        kind: Some(NoticeKind::FilesChangedNoCopy { files, more }),
    })
}

#[cfg(test)]
mod tests;
