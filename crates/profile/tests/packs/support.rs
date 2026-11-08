//! The std calls `clippy.toml` bans, as thin wrappers and nothing else: the file changes the
//! pack fixtures need, and the clock the timing test reads. The expect below covers only this
//! file.

#![expect(
    clippy::disallowed_methods,
    reason = "the pack tests' fixtures and timing call std's writes and clock here alone; clippy.toml bans them elsewhere"
)]

use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// A fresh, empty folder at `path`.
pub(crate) fn fresh_dir(path: &Path) -> io::Result<()> {
    remove_dir_all(path)?;
    fs::create_dir_all(path)
}

/// `fs::create_dir_all`.
pub(crate) fn make_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

/// `fs::write`, making the folder first.
pub(crate) fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, contents)
}

/// `fs::remove_file`.
#[cfg(unix)]
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

/// `fs::remove_dir_all`; a missing folder is no error.
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// `fs::hard_link`.
pub(crate) fn hard_link(original: &Path, link: &Path) -> io::Result<()> {
    fs::hard_link(original, link)
}

/// A symlink at `link` to `target`.
#[cfg(unix)]
pub(crate) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// A symlink at `link` to the folder `target`; fails without the privilege to make one.
#[cfg(windows)]
pub(crate) fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

/// Sets the file's Unix mode.
#[cfg(unix)]
pub(crate) fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// How long `run` takes.
pub(crate) fn timed<T>(run: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let out = run();
    (out, start.elapsed())
}
