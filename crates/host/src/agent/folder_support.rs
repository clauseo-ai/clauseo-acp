//! The Windows folder check of a matter folder: whether Codex's sandbox can add the permission
//! entries it needs there before Codex may write (`clauseo_sandbox::windows::folder_support`).
//! It runs off the actor only: on the create's worker (`matter_worker.rs`), in a Codex
//! launch's preparation ([`super::codex::prepare_launch`]), right after the `.codex` refusal,
//! and in a Claude launch's (`actor/turn/launch_checks.rs`), where it never refuses but decides
//! forced ask (`agent/forced_ask.rs`). Its answer goes to the actor's memory for the matter's
//! row (`Actor::folder_supports`).
//!
//! A launch decides on its own answer, never on the remembered one: `network` and
//! `no_permissions` refuse Codex (`matter.folder_network`, `matter.folder_no_permissions`);
//! `cant_change_permissions` starts Codex and only shows on the row. A check that fails is a
//! warning and an unknown answer ([`Answer::Failed`]), never a Codex refusal (Codex's sandbox
//! stays the boundary) and nothing on the row; a Claude launch fails closed into forced ask. A
//! folder that is not found is left to the other folder checks. Off Windows nothing is checked
//! and there is never an answer.
//!
//! Each check logs one line with the matter and the answer, never the folder (ADR 0047).

use std::path::Path;

use clauseo_protocol::{ErrorCode, FolderSupportKind, MatterId};

use crate::error::Error;

/// A folder check's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    /// The check answered: `None` for a fine folder, and always off Windows.
    Known(Option<FolderSupportKind>),
    /// The check failed (an io error, `NotFound` included): unknown. Never a refusal, and the
    /// row shows nothing; a Claude launch fails closed into forced ask (`agent/forced_ask.rs`).
    Failed,
}

impl Answer {
    /// The kind the matter's row shows: none for a failed check.
    pub(crate) fn kind(self) -> Option<FolderSupportKind> {
        match self {
            Self::Known(kind) => kind,
            Self::Failed => None,
        }
    }
}

/// The folder check of `matter`'s folder `folder`. A failure is a warning with the io error's
/// kind (a missing folder an info line: the other folder checks report it).
#[cfg(windows)]
pub(crate) fn check(matter: MatterId, folder: &Path) -> Answer {
    use clauseo_sandbox::windows::{FolderSupport, folder_support};
    match folder_support(folder) {
        Ok(support) => {
            let kind = match support {
                FolderSupport::Ok => None,
                FolderSupport::CantChangePermissions => {
                    Some(FolderSupportKind::CantChangePermissions)
                }
                FolderSupport::Network => Some(FolderSupportKind::Network),
                FolderSupport::NoPermissions => Some(FolderSupportKind::NoPermissions),
            };
            tracing::info!(%matter, support = ?kind, "the matter folder was checked");
            Answer::Known(kind)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(%matter, "the matter folder check found no folder; the others report it");
            Answer::Failed
        }
        Err(error) => {
            // The kind only: the error's text may hold the path.
            tracing::warn!(
                %matter,
                error_kind = ?error.kind(),
                "the matter folder could not be checked: Codex is not refused, Claude asks first"
            );
            Answer::Failed
        }
    }
}

/// Off Windows there is no check: the sandbox needs no permission entries there.
#[cfg(not(windows))]
pub(crate) fn check(_matter: MatterId, _folder: &Path) -> Answer {
    Answer::Known(None)
}

/// Where a launch's folder check gets its answer: the Windows check, or in tests a fixed answer
/// (`HostConfig::with_folder_support` and `with_folder_check_failed`, test-support only), so the
/// policies that follow from it run on every OS. A release build has only the Windows check.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum CheckSource {
    /// [`check`]: the Windows check, and nothing off Windows.
    #[default]
    Windows,
    /// This answer for every launch, logged as a check is.
    #[cfg(feature = "test-support")]
    Fixed(Answer),
}

/// One launch's folder check of `matter`'s folder, from `source`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FolderCheck {
    pub(crate) matter: MatterId,
    pub(crate) source: CheckSource,
}

impl FolderCheck {
    /// The answer for `folder`. Its file system calls: off the actor.
    pub(crate) fn run(&self, folder: &Path) -> Answer {
        match self.source {
            CheckSource::Windows => check(self.matter, folder),
            #[cfg(feature = "test-support")]
            CheckSource::Fixed(answer) => {
                let matter = self.matter;
                tracing::info!(%matter, ?answer, "the matter folder was checked (fixed)");
                answer
            }
        }
    }

    /// The check at a Codex launch: the answer, or the refusal when Codex can't work in the
    /// folder at all. A failed check is no refusal: Codex's own sandbox applies.
    pub(crate) fn refuse_unsupported(&self, folder: &Path) -> Result<Answer, Error> {
        match self.run(folder) {
            Answer::Known(kind) => decide(kind).map(Answer::Known),
            Answer::Failed => Ok(Answer::Failed),
        }
    }
}

/// What a Codex launch does with the answer `kind`: a refusal for `network` and
/// `no_permissions`, which name no path; otherwise the answer, for the matter's row.
pub(crate) fn decide(kind: Option<FolderSupportKind>) -> Result<Option<FolderSupportKind>, Error> {
    match kind {
        Some(FolderSupportKind::Network) => Err(Error::refused(
            ErrorCode::MatterFolderNetwork,
            "Codex can't work in this matter: its folder is on a network drive.",
        )),
        Some(FolderSupportKind::NoPermissions) => Err(Error::refused(
            ErrorCode::MatterFolderNoPermissions,
            "Codex can't work in this matter: its folder's drive keeps no file permissions.",
        )),
        other => Ok(other),
    }
}

/// The answer a launch refusal carries, for the matter's row: only the two codes [`decide`]
/// refuses with name one.
pub(crate) fn of_refusal(code: ErrorCode) -> Option<FolderSupportKind> {
    if code == ErrorCode::MatterFolderNetwork {
        Some(FolderSupportKind::Network)
    } else if code == ErrorCode::MatterFolderNoPermissions {
        Some(FolderSupportKind::NoPermissions)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
