//! The std calls that change files, as thin wrappers and nothing else: the one place the row
//! copies the shell and the helper into its test root, writes the artifacts and chrome's
//! standard error log, and deletes the root.
//! Workspace `clippy.toml` bans them, tests included, and the expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the proof row changes its own folders here alone; clippy.toml bans it elsewhere"
)]

use std::io;
use std::path::Path;

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// `fs::copy`: the copy takes the folder's inherited ACEs, not the source's.
pub(crate) fn copy(from: &Path, to: &Path) -> io::Result<u64> {
    std::fs::copy(from, to)
}

/// `fs::write`.
pub(crate) fn write(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    std::fs::write(path, contents)
}

/// `File::create`: the helper's file for chrome's standard error.
pub(crate) fn create(path: &Path) -> io::Result<std::fs::File> {
    std::fs::File::create(path)
}

/// `fs::remove_dir_all`.
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// Copies the files and folders inside `from` into the existing folder `to`, returning how many
/// files it copied. Links are skipped.
pub(crate) fn copy_tree(from: &Path, to: &Path) -> io::Result<u64> {
    let mut files = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            std::fs::create_dir(&target)?;
            files += copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)?;
            files += 1;
        }
    }
    Ok(files)
}
