//! Copying a tree into a folder, and setting the times of one entry under a folder.
//!
//! A copy keeps each entry's access and modification times: they are set from the original after
//! the entry is made, never left to the platform's copy. A symlink stays a symlink to the same
//! target on Unix; elsewhere it fails the copy. A file is copied with `fs::copy`, which on Apple
//! platforms clones it where the file system can (std 1.99, `library/std/src/sys/fs/unix.rs`,
//! Apple's `copy`: `fclonefileat` first, then `fcopyfile` when the clone fails with ENOTSUP,
//! EEXIST or EXDEV), so a copy on APFS shares the original's blocks; elsewhere it copies the
//! bytes. A file's permissions come along with it; a folder is made with the default ones.

use super::{plain_name, sys};
use std::fs::{self, FileTimes, Metadata};
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// Copies `from`, a file, a folder with everything under it, or a symlink, to `<dir>/<name>`,
/// making `dir` and its missing parents first. Refuses a `name` that is not one plain file name,
/// and a `<dir>/<name>` that is already there, so nothing is replaced. A symlink is copied as a
/// symlink (on Unix), never followed. A folder's times are set once its entries are in.
pub(crate) fn copy_into(from: &Path, dir: &Path, name: &str) -> io::Result<()> {
    plain_name(name, "name in the folder")?;
    let to = dir.join(name);
    if fs::symlink_metadata(&to).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "{} is already there, so nothing is copied over it",
                to.display()
            ),
        ));
    }
    fs::symlink_metadata(from)?;
    sys::create_dir_all(dir)?;
    copy_entry(from, &to)
}

/// Copies the entry `from` to `to`, which is not there yet, with its times.
fn copy_entry(from: &Path, to: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    let kind = meta.file_type();
    if kind.is_symlink() {
        copy_link(from, to)?;
        sys::set_times_nofollow(to, times_of(&meta)?)
    } else if kind.is_dir() {
        sys::create_dir(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_entry(&entry.path(), &to.join(entry.file_name()))?;
        }
        sys::set_times(to, times_of(&meta)?)
    } else if kind.is_file() {
        sys::copy(from, to)?;
        sys::set_times(to, times_of(&meta)?)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} is not a file, a folder or a symlink, so it is not copied",
                from.display()
            ),
        ))
    }
}

/// The access and modification times of `meta`.
fn times_of(meta: &Metadata) -> io::Result<FileTimes> {
    Ok(FileTimes::new()
        .set_accessed(meta.accessed()?)
        .set_modified(meta.modified()?))
}

/// Makes `to` a symlink with the target of the symlink `from`. On Windows a symlink is refused
/// instead: Windows tells file links from folder links, and cargo's build output there has none.
fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    let target = fs::read_link(from)?;
    #[cfg(unix)]
    let made = sys::symlink(&target, to);
    #[cfg(not(unix))]
    let made = Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "{} is a symlink to {}; xtask copies no symlink to {} on this OS",
            from.display(),
            target.display(),
            to.display()
        ),
    ));
    made
}

/// Sets the access and modification times of `<dir>/<path>` to `time`. `path` is relative, its
/// parts plain names joined by `/`, as `git ls-files` prints a path; each part is joined on its
/// own, so a Windows folder gets `\` between them. When the last part is a symlink, it gets its
/// own times and is not followed. Refuses any other `path` (absolute, with `.`, `..`, an empty
/// part or a `\`), so `path` alone cannot name a file outside `dir`; a symlinked folder earlier
/// in `path` is followed, wherever it points. `git ls-files` never lists a path under a tracked
/// symlink, so the worktree run does not reach that case.
pub(crate) fn set_times_in(dir: &Path, path: &str, time: SystemTime) -> io::Result<()> {
    let mut full = dir.to_path_buf();
    for part in path.split('/') {
        if plain_name(part, "part").is_err() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{path}` is not a relative path of plain names joined by /"),
            ));
        }
        full.push(part);
    }
    let times = FileTimes::new().set_accessed(time).set_modified(time);
    sys::set_times_nofollow(&full, times)
}

#[cfg(test)]
mod tests;
