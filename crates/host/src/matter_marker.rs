//! The marker check for the desktop's `open_file` and `reveal`
//! (`docs/decisions/0026-open-and-reveal.md`), which show or open what is in a matter's folder
//! without opening the matter: [`check_matter_folder`].

use std::path::Path;

use clauseo_protocol::{ErrorCode, HostError, MatterId};

/// Whether `folder` is still matter `matter`'s folder, by its `.clauseo/matter.json`
/// (`clauseo_matters::check_marker`): `matter.folder_missing` when the folder is not there,
/// `matter.folder_not_a_matter` when it is there but its marker is missing, does not read, or
/// names another matter. The message names the reason, never the path. A folder or marker that
/// does not read for another reason (permissions, an offline placeholder) is logged, by its code
/// alone, and let through: the open or reveal then answers for itself. Reads the marker alone,
/// on the caller's thread.
pub fn check_matter_folder(folder: &Path, matter: MatterId) -> Result<(), HostError> {
    match clauseo_matters::check_marker(folder, matter) {
        Ok(()) => Ok(()),
        Err(error @ clauseo_matters::Error::FolderMissing { reason, .. }) => Err(HostError::new(
            error.code(),
            format!("the folder of matter {matter}: {reason}"),
        )),
        Err(error) => {
            let code: ErrorCode = error.code();
            tracing::warn!(code = %code, matter = %matter, "a matter's marker did not read; the open goes ahead");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
