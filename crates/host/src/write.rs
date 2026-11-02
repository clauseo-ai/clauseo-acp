//! The host's file system writes: the one module that may call std's write functions.

#![expect(
    clippy::disallowed_methods,
    reason = "the host's one place that creates folders (the data folder, the matters root and \
              agents' homes), writes an agent home's config.toml, and writes unit tests' temp \
              folders"
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

/// Replaces the file `path` with `bytes` atomically: a fresh temp file beside it (0600 on Unix),
/// written and synced, then renamed over `path`. A link at `path` is replaced, never followed.
pub(crate) fn replace_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut temp = path.as_os_str().to_owned();
    temp.push(".clauseo-new");
    let temp = std::path::PathBuf::from(temp);
    match std::fs::remove_file(&temp) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        Ok(()) | Err(_) => {}
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options.open(&temp).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    let renamed = written.and_then(|()| std::fs::rename(&temp, path));
    if renamed.is_err() {
        // What is left of the temp file is ours; the error that matters is the write's.
        drop(std::fs::remove_file(&temp));
    }
    renamed
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
