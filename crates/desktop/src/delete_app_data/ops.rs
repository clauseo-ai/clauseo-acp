//! The deletion's only changes to the file system. The desktop crate has no write module; these
//! wrappers are the one place `--delete-app-data` removes or changes anything.
#![expect(
    clippy::disallowed_methods,
    reason = "the uninstaller's deletion of the data folders: these wrappers are its only writes"
)]

use std::fs::Metadata;
use std::io;
use std::path::Path;

use super::walk::Report;

/// Removes the reparse point `path` as a link, never what it points at: `remove_dir` when the
/// link itself is a directory one (a junction, a directory symbolic link, a mount point), else
/// `remove_file`. `meta` is `path`'s own, from `symlink_metadata`.
#[cfg(windows)]
pub(super) fn remove_link(path: &Path, meta: &Metadata) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt as _;
    /// `FILE_ATTRIBUTE_DIRECTORY`.
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    if meta.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Removes the symbolic link `path` (`unlink`), never what it points at.
#[cfg(not(windows))]
pub(super) fn remove_link(path: &Path, _meta: &Metadata) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// Removes the file `path`.
pub(super) fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// Removes the empty folder `path`.
pub(super) fn remove_dir(path: &Path) -> io::Result<()> {
    std::fs::remove_dir(path)
}

/// Removes the folder `path` and everything in it with std, which never follows a link.
pub(super) fn remove_dir_all(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// Clears the read-only attribute of `path`, which must be no link.
pub(super) fn clear_read_only(path: &Path, report: &mut Report<'_>) {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        clear_read_only_meta(path, &meta, report);
    }
}

/// Clears the read-only attribute of `path`, no link, whose own metadata is `meta`. A failure
/// is written, not counted: the removal that follows reports it if it matters.
#[cfg(windows)]
pub(super) fn clear_read_only_meta(path: &Path, meta: &Metadata, report: &mut Report<'_>) {
    let mut permissions = meta.permissions();
    if !permissions.readonly() {
        return;
    }
    permissions.set_readonly(false);
    if let Err(error) = std::fs::set_permissions(path, permissions) {
        report.note(&format!(
            "{}: could not clear read-only ({error})",
            path.display()
        ));
    }
}

/// Off Windows there is no read-only attribute to clear, and a mode is never changed.
#[cfg(not(windows))]
pub(super) fn clear_read_only_meta(_path: &Path, _meta: &Metadata, _report: &mut Report<'_>) {}
