//! A matter job's kind, taken before the job runs: its name for the log, its clean-up and its
//! reply when it panics or does not run (`crash.rs`).

use std::path::PathBuf;

use crate::agent::data_fence::PathsChecked;
use crate::error::Error;
use crate::matter_worker::{MatterJob, MatterReply};
use crate::revisions::{Act, RevisionsDone};

/// A job's kind: its name for the log, and its reply when it cannot run.
#[derive(Debug)]
pub(super) enum Kind {
    Create,
    Import,
    Open,
    Locate,
    BeforeTurn,
    AfterTurn,
    Repair,
    Undo,
    Restore,
    RestoreRecord,
    Compare,
    Preview,
    HeldOpen,
    ListPapers,
    EditedSince,
    StillGone,
    Discard,
    WriteSides,
    /// The job's private folder, removed when the job does not finish, as `revisions::run`
    /// removes it after a job that does.
    Revisions {
        decide: bool,
        work: PathBuf,
    },
}

impl Kind {
    pub(super) fn of(job: &MatterJob) -> Self {
        match job {
            MatterJob::Create { .. } => Self::Create,
            MatterJob::Import { .. } => Self::Import,
            MatterJob::Open { .. } => Self::Open,
            MatterJob::Locate { .. } => Self::Locate,
            MatterJob::BeforeTurn(_) => Self::BeforeTurn,
            MatterJob::AfterTurn { .. } => Self::AfterTurn,
            MatterJob::Repair { .. } => Self::Repair,
            MatterJob::Undo(_) => Self::Undo,
            MatterJob::Restore(_) => Self::Restore,
            MatterJob::RestoreRecord(_) => Self::RestoreRecord,
            MatterJob::Compare { .. } => Self::Compare,
            MatterJob::Preview { .. } => Self::Preview,
            MatterJob::HeldOpen { .. } => Self::HeldOpen,
            MatterJob::ListPapers { .. } => Self::ListPapers,
            MatterJob::EditedSince { .. } => Self::EditedSince,
            MatterJob::StillGone { .. } => Self::StillGone,
            MatterJob::Discard => Self::Discard,
            MatterJob::WriteSides(_) => Self::WriteSides,
            MatterJob::Revisions(job) => Self::Revisions {
                decide: matches!(job.act, Act::Decide { .. }),
                work: job.work.clone(),
            },
        }
    }

    /// The name for the log: the job's fields hold paths.
    pub(super) const fn name(&self) -> &'static str {
        match self {
            Self::Create => "Create",
            Self::Import => "Import",
            Self::Open => "Open",
            Self::Locate => "Locate",
            Self::BeforeTurn => "BeforeTurn",
            Self::AfterTurn => "AfterTurn",
            Self::Repair => "Repair",
            Self::Undo => "Undo",
            Self::Restore => "Restore",
            Self::RestoreRecord => "RestoreRecord",
            Self::Compare => "Compare",
            Self::Preview => "Preview",
            Self::HeldOpen => "HeldOpen",
            Self::ListPapers => "ListPapers",
            Self::EditedSince => "EditedSince",
            Self::StillGone => "StillGone",
            Self::Discard => "Discard",
            Self::WriteSides => "WriteSides",
            Self::Revisions { .. } => "Revisions",
        }
    }

    /// After a job that did not finish: a tracked-changes job's private folder is removed, as
    /// `revisions::run` removes it after one that does.
    pub(super) fn clean_up(&self) {
        let Self::Revisions { work, .. } = self else {
            return;
        };
        if let Err(err) = crate::write::remove_tree(work) {
            tracing::warn!(
                error = %crate::log_file::Cause(&err),
                "a tracked-changes work folder was left behind"
            );
        }
    }

    /// The job's reply when it panicked or did not run, holding `error`; builds it only.
    /// `HeldOpen` has no error to hold: it reads as nothing held, so the permission request
    /// goes to the lawyer, as it does when the job cannot be sent (`turn/asks.rs`).
    pub(super) fn reply(&self, error: Error) -> MatterReply {
        match self {
            Self::Create => MatterReply::Created(Err(error)),
            Self::Import => MatterReply::Imported(Err(error)),
            Self::Open => MatterReply::Opened(Err(error)),
            Self::Locate => MatterReply::Located(Err(error)),
            Self::BeforeTurn => MatterReply::BeforeTurn(Err(error)),
            Self::AfterTurn | Self::Repair => MatterReply::AfterTurn(Err(error)),
            Self::Undo => MatterReply::Undone(Err(error)),
            Self::Restore => MatterReply::Restored(Err(error)),
            Self::RestoreRecord => MatterReply::RestoreRecorded(Err(error)),
            Self::Compare => MatterReply::Compared(Err(error)),
            Self::Preview => MatterReply::Previewed(Err(error)),
            Self::HeldOpen => MatterReply::HeldOpen(PathsChecked::Held(None)),
            Self::ListPapers => MatterReply::Papers(Err(error)),
            Self::EditedSince => MatterReply::EditedSince(Err(error)),
            Self::StillGone => MatterReply::StillGone(Err(error)),
            Self::Discard => MatterReply::Discarded(Err(error)),
            Self::WriteSides => MatterReply::SidesWritten(Err(error)),
            Self::Revisions { decide, .. } => MatterReply::Revisions(Box::new(if *decide {
                RevisionsDone::Decided(Err(error))
            } else {
                RevisionsDone::Listed(Err(error))
            })),
        }
    }
}
