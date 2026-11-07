//! A forgotten matter's set-aside bookkeeping, deleted by the per-user `--remove` as the owner
//! (`setup/remove/ledger_work.rs`), never on the administrator's path (K1). Every launch leaves
//! `<data_dir>\sandbox\set-aside\<matter>\record.json` and `set-aside.lock`
//! ([`SET_ASIDE_BOOKKEEPING`]); once the matter is forgotten they record nothing anyone reads.
//! Only those two names go, each only as a plain file (no reparse point, no folder), then the
//! matter's folder, only when nothing else is in it: the lawyer's files a pass moved aside, and
//! the manifest listing them, stay, and the uninstaller keeps the set-aside for them.
//!
//! On NTFS everything is opened by handle without following a link (`ntfs/forget.rs`); off
//! Windows, where only the tests run it, a link or anything but a plain file is refused by its
//! own metadata.

use std::path::Path;

use clauseo_protocol::MatterId;

use super::{SET_ASIDE_BOOKKEEPING, SetAsideFailure};
use crate::windows::matters::SANDBOX_DIR;

/// Deletes `matter`'s set-aside bookkeeping under `data_dir` (a data folder already checked),
/// then the matter's folder when it is empty; answers whether that folder went. A matter with no
/// folder is `Ok(false)`. A bookkeeping name that is a link or a folder, or a folder on the way
/// that is a link, is [`SetAsideFailure::Changed`] and nothing more is deleted.
pub(crate) fn forget_bookkeeping(
    data_dir: &Path,
    matter: &MatterId,
) -> Result<bool, SetAsideFailure> {
    let sandbox = data_dir.join(SANDBOX_DIR);
    let name = matter.to_string();
    #[cfg(windows)]
    return super::ntfs::forget_bookkeeping(
        &sandbox,
        std::ffi::OsStr::new(&name),
        &SET_ASIDE_BOOKKEEPING,
    );
    #[cfg(not(windows))]
    posix(&sandbox.join(super::SET_ASIDE_DIR).join(name))
}

/// The tests' stand-in off Windows: each name checked by its own metadata (a symbolic link or
/// anything but a file refused), removed through `write::remove_owned_file`, then the folder
/// with `rmdir`, which removes only an empty one.
#[cfg(not(windows))]
fn posix(folder: &Path) -> Result<bool, SetAsideFailure> {
    use std::io;

    use super::files::io_failure;
    use crate::windows::write;
    let plain = |path: &Path, folder: bool| match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || meta.is_dir() != folder => {
            Err(SetAsideFailure::Changed)
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_failure(&error)),
    };
    if !plain(folder, true)? {
        return Ok(false);
    }
    for name in SET_ASIDE_BOOKKEEPING {
        let path = folder.join(name);
        if plain(&path, false)? {
            write::remove_owned_file(&path).map_err(|error| io_failure(&error))?;
        }
    }
    match write::remove_empty_folder(folder) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => Ok(false),
        Err(error) => Err(io_failure(&error)),
    }
}
