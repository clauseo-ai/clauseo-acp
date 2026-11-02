//! The folders a new matter's folder must not be, hold or sit inside, and the check an import
//! runs on its worker after the place check (`MatterJob::Import`).

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use crate::error::Error;

/// How a folder of the registry is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Claim {
    /// A matter on Clauseo's list.
    Listed,
    /// A matter being made by a create or import in flight, or a relocate's target.
    Making,
    /// A forgotten matter: its own folder brings it back on import.
    Forgotten,
}

/// One folder a new matter's folder must not be, hold or sit inside.
#[derive(Debug, Clone)]
pub(crate) struct Claimed {
    pub(crate) matter: MatterId,
    pub(crate) folder: PathBuf,
    pub(crate) claim: Claim,
}

/// `matter.folder_is_matter` when `folder` is, holds or sits inside a folder of `registry`,
/// compared as the refused-folder check compares (`same_or_nested`). The error names the matter
/// (`HostError::matter`) only when it is on the list: there is nothing to open otherwise. For an
/// import (`import`), a forgotten matter's own folder passes: the import finds its marker and
/// brings that matter back.
pub(crate) fn check_registry(
    folder: &Path,
    registry: &[Claimed],
    import: bool,
) -> Result<(), Error> {
    for claimed in registry {
        if !clauseo_matters::same_or_nested(folder, &claimed.folder) {
            continue;
        }
        if import && claimed.claim == Claim::Forgotten && same_folder(folder, &claimed.folder) {
            continue;
        }
        let refusal = clauseo_protocol::HostError::new(
            clauseo_protocol::ErrorCode::MatterFolderIsMatter,
            format!(
                "{} is, holds or sits inside the folder of matter {} ({:?})",
                folder.display(),
                claimed.matter,
                claimed.claim
            ),
        );
        let refusal = match claimed.claim {
            Claim::Listed => refusal.with_matter(claimed.matter),
            Claim::Making | Claim::Forgotten => refusal,
        };
        return Err(Error::Refused(refusal));
    }
    Ok(())
}

/// Whether two folders that `same_or_nested` relates are the same folder, not one inside the
/// other: their paths, canonicalised (as written when that fails), have as many components.
fn same_folder(a: &Path, b: &Path) -> bool {
    let depth = |path: &Path| {
        dunce::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .components()
            .count()
    };
    depth(a) == depth(b)
}
