//! Lock files: a file whose exclusive lock one run holds, so a second run of the same tool sees it.
//! The OS drops the lock when the process ends, killed or not.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Opens the lock file at `path` for reading and writing, creating it and its folders when
/// missing. An existing file is not truncated.
pub(crate) fn create_lock_file(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

/// Opens the existing lock file at `path` for reading and writing, to test its lock. A missing
/// file is never created: the open fails with `NotFound`.
pub(crate) fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open(path)
}
