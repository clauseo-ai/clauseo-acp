//! xtask's one door for changing files. Workspace `clippy.toml` bans every std call that changes
//! the file system (`disallowed-methods`); `sys.rs` holds the only calls to them, each a thin
//! wrapper, and is the only file whose expect lifts the ban. Every operation here is confined to a
//! folder it is given and refuses a name that would leave it; it knows no tool's paths and hands
//! out no `File` to write through. A tool keeps its own paths and passes them in. xtask's tests
//! write through `Scratch` (`scratch.rs`).
//!
//! - `write_file_in`, `remove_in`: write one file directly in a folder, remove one entry directly
//!   in a folder.
//! - `LockFile::open`: a lock file directly in a folder, whose exclusive lock keeps a second run
//!   of a tool out.
//! - `Guard`, `Backups`: the tracked-file guard. Any tool that edits a tracked file saves its
//!   original, writes, and puts it back through `Guard`, and recovers an interrupted run's copies
//!   with `Backups`.
//! - `Staging`: a staging dir in a bin folder that downloads go to, and the install from it.

mod guard;
mod lock;
mod replace;
mod staging;
mod sys;

pub(crate) use guard::{Backups, Guard, GuardFailure};
pub(crate) use lock::LockFile;
pub(crate) use staging::Staging;

use std::io;
use std::path::{Component, Path};

/// Writes `bytes` to `<dir>/<name>`, making `dir` and its missing parents first and replacing a
/// file already there. Refuses a `name` that is not one plain file name, so the write never
/// leaves `dir`.
pub(crate) fn write_file_in(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    plain_name(name, "file name")?;
    sys::create_dir_all(dir)?;
    sys::write(&dir.join(name), bytes)
}

/// Removes `<dir>/<name>`: the folder and everything in it, or the file. A symlink is removed,
/// never followed. Refuses a `name` that is not one plain file name, so nothing outside `dir`, or
/// deeper in it, is removed. A missing entry fails with `NotFound`.
pub(crate) fn remove_in(dir: &Path, name: &str) -> io::Result<()> {
    plain_name(name, "name in the folder")?;
    let path = dir.join(name);
    if std::fs::symlink_metadata(&path)?.is_dir() {
        sys::remove_dir_all(&path)
    } else {
        sys::remove_file(&path)
    }
}

/// `Ok` when `name` is one plain file name (no folder, no `..`, no root, no `\`), else the
/// refusal naming `what` it was passed as.
fn plain_name(name: &str, what: &str) -> io::Result<()> {
    let mut parts = Path::new(name).components();
    if matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    ) && !name.contains('\\')
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("`{name}` is not a {what}: it must be one plain file name"),
        ))
    }
}

#[cfg(test)]
mod tests;
