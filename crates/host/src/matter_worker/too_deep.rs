//! The Windows path-length refusal of a matter folder, moved out of `matter_worker.rs` to keep
//! that file within its size limit. It answers whether a folder is too deep to start programs in.

use std::path::Path;

use clauseo_protocol::ErrorCode;

use crate::error::Error;

/// Refuses `folder` when Windows can't start programs in it: `matter.folder_too_deep`, on
/// Windows only (`clauseo_matters::is_too_deep`). An open checks it before the matter is opened,
/// and a turn's launch before its agent's home or any session folder is made, so a matter moved
/// or renamed deeper is refused where it is opened or launched, and stays listed. Its file
/// system calls: none.
pub(crate) fn refuse_too_deep(folder: &Path) -> Result<(), Error> {
    if clauseo_matters::is_too_deep(folder) {
        return Err(Error::refused(
            ErrorCode::MatterFolderTooDeep,
            "the matter's path is too long for Windows to start programs in it",
        ));
    }
    Ok(())
}
