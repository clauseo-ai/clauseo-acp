//! The std calls that change files or walk `PATH`, as thin wrappers and nothing else: the one
//! place the install tests change their own folders and look for the real Node. Workspace
//! `clippy.toml` bans them, tests included, and the expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the install tests change their folders and walk PATH here alone"
)]

use std::io;
use std::path::{Path, PathBuf};

/// `fs::write`, making the file's folder first.
pub(crate) fn write(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

/// `fs::write`, then mode 0755: a script the launcher can start.
#[cfg(unix)]
pub(crate) fn write_executable(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    write(path, contents)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

/// The first folder on this process's `PATH` holding `name`.
pub(crate) fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Opens `path` (made if missing) and takes its exclusive lock, as another Clauseo would.
pub(crate) fn hold_lock(path: &Path) -> io::Result<std::fs::File> {
    let file = open_lock(path)?;
    file.lock()?;
    Ok(file)
}

/// Opens `path` (made if missing) and takes its shared lock, as another Clauseo installing
/// holds `agents/.lock`.
pub(crate) fn hold_shared(path: &Path) -> io::Result<std::fs::File> {
    let file = open_lock(path)?;
    file.lock_shared()?;
    Ok(file)
}

fn open_lock(path: &Path) -> io::Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
}

/// A file of `len` bytes at `path` that takes no room (sparse, on Unix's file systems), made
/// with its folders: what a large install has written, for the disk check's tests.
#[cfg(unix)]
pub(crate) fn sparse(path: &Path, len: u64) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::File::create(path)?.set_len(len)
}
