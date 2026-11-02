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
