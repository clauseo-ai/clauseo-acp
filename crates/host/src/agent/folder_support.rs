//! The Windows folder check of a matter folder: whether Codex's sandbox can add the permission
//! entries it needs there before Codex may write (`clauseo_sandbox::windows::folder_support`).
//! It runs off the actor only: on the create's worker (`matter_worker.rs`) and in a Codex
//! launch's preparation ([`super::codex::prepare_launch`]), right after the `.codex` refusal.
//! Its answer goes to the actor's memory for the matter's row (`Actor::folder_supports`).
//!
//! A launch decides on its own answer, never on the remembered one: `network` and
//! `no_permissions` refuse Codex (`matter.folder_network`, `matter.folder_no_permissions`);
//! `cant_change_permissions` starts Codex and only shows on the row. A check that fails is a
//! warning and no answer, never a refusal: this check tells the lawyer why Codex struggles, and
//! Codex's sandbox stays the boundary. A folder that is not found is left to the other folder
//! checks. Off Windows nothing is checked and there is never an answer.
//!
//! Each check logs one line with the matter and the answer, never the folder (ADR 0047).

use std::path::Path;

use clauseo_protocol::{ErrorCode, FolderSupportKind, MatterId};

use crate::error::Error;

/// The folder check of `matter`'s folder `folder`: `None` when the folder is fine, when the
/// check failed (a warning), and off Windows, where nothing is checked.
#[cfg(windows)]
pub(crate) fn check(matter: MatterId, folder: &Path) -> Option<FolderSupportKind> {
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
            tracing::info!(%matter, support = ?kind, "the matter folder was checked for Codex");
            kind
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(%matter, "the matter folder check found no folder; the others report it");
            None
        }
        Err(error) => {
            // The kind only: the error's text may hold the path.
            tracing::warn!(
                %matter,
                error_kind = ?error.kind(),
                "the matter folder could not be checked for Codex; Codex is not refused for it"
            );
            None
        }
    }
}

/// Off Windows there is no check: the sandbox needs no permission entries there.
#[cfg(not(windows))]
pub(crate) fn check(_matter: MatterId, _folder: &Path) -> Option<FolderSupportKind> {
    None
}

/// The folder check at a Codex launch: the answer, or the refusal when Codex can't work in the
/// folder at all.
pub(crate) fn refuse_unsupported_folder(
    matter: MatterId,
    folder: &Path,
) -> Result<Option<FolderSupportKind>, Error> {
    decide(check(matter, folder))
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
