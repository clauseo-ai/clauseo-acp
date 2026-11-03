//! The std calls that create and remove files, as thin wrappers and nothing else: the one place
//! in `search` that may call them. Workspace `clippy.toml` bans them everywhere else, and the
//! expect below covers only this file. SQLite writes the database itself; these make the empty
//! file with the private mode before SQLite opens it, and remove a file that is to be rebuilt.

#![expect(
    clippy::disallowed_methods,
    reason = "search creates its private index file and removes one to rebuild here alone; \
              clippy.toml bans std's writes elsewhere"
)]

use std::fs::OpenOptions;
use std::io;
use std::path::Path;

/// Creates `path` empty with mode `0o600` on Unix when it is missing. An existing file keeps its
/// bytes and its mode.
pub(crate) fn create_private(path: &Path) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map(drop)
}

/// Removes `path`; a file that is already gone is not an error.
pub(crate) fn remove_if_present(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
