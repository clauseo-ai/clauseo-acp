//! The host's file system writes: the one module that may call std's write functions.

#![expect(
    clippy::disallowed_methods,
    reason = "the host's one place that creates folders (the data folder, the matters root and \
              agents' homes) and writes unit tests' temp folders"
)]

use std::io;
use std::path::Path;

/// Creates `path` and every missing parent; an existing folder is fine.
pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Creates `path` and every missing parent, `path` itself private to the user on Unix (0700,
/// and set to 0700 when it already exists): an agent's home, which holds its login.
pub(crate) fn create_private_dir_all(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    match builder.create(path) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        other => other?,
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_dir() && meta.permissions().mode() & 0o777 != 0o700 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

/// Removes `path` and everything under it; a missing folder is fine. For unit tests' temp folders.
#[cfg(test)]
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Writes `bytes` to the file `path`, replacing it. For unit tests' fixtures.
#[cfg(test)]
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    std::fs::write(path, bytes)
}

/// Renames `from` to `to`. For unit tests: the lawyer moving a folder.
#[cfg(test)]
pub(crate) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Removes the file `path`. For unit tests.
#[cfg(test)]
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// Makes `link` a symbolic link to `target`. For unit tests: a matter stored as a link.
#[cfg(all(test, unix))]
pub(crate) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}
