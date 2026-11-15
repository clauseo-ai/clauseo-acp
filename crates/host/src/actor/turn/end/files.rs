//! The after-turn checkpoint's part of a turn's close: the matter's `CheckpointRecorded`, the
//! turn's `FilesChanged`, and the notice for files gone with no copy.
//!
//! A file the before-turn checkpoint skipped (cloud-only, or over the size cap) has no version,
//! so its deletion is no change, and no undo brings it back. The checkpoint lists such files as
//! `gone` (`clauseo_matters`' `gone.rs`); the close logs them in `FilesChanged.gone`, which
//! Versions shows on the step, and as a `Notice` (`NoticeKind::FilesGoneNoCopy`) right after it,
//! which the conversation shows. Both come from the same list, so they cannot disagree.

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

/// How many gone files a notice names; the rest are counted.
const NAMED: usize = 5;

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
/// at most [`NAMED`] named (sorted by path bytes, as listed), the rest counted. Its text, for a
/// view that does not know the kind, is English.
pub(in super::super) fn gone_notice(gone: &[SkippedFile]) -> Option<Notice> {
    let first = gone.first()?;
    let text = if gone.len() == 1 {
        format!(
            "{} was deleted during this step. Clauseo had no copy of it to bring back.",
            first.path
        )
    } else {
        format!(
            "{} files were deleted during this step that Clauseo had no copy of, so they can't be \
             brought back.",
            gone.len()
        )
    };
    let more = u32::try_from(gone.len().saturating_sub(NAMED)).unwrap_or(u32::MAX);
    let files = gone.iter().take(NAMED).cloned().collect();
    Some(Notice {
        level: NoticeLevel::Warning,
        text,
        kind: Some(NoticeKind::FilesGoneNoCopy { files, more }),
    })
}

#[cfg(test)]
mod tests;
