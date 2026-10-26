//! The std calls that create files, as thin wrappers and nothing else: the one place in `store`
//! that may call them. Workspace `clippy.toml` bans them everywhere else, and the expect below
//! covers only this file. SQLite writes the database itself; these make the lock file and the
//! empty database file with the private mode before SQLite opens it.

#![expect(
    clippy::disallowed_methods,
    reason = "store creates its lock file and its private database file here alone; clippy.toml \
              bans std's writes elsewhere"
)]

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Opens `path` for reading and writing, creating it empty with mode `0o600` on Unix when it is
/// missing. An existing file keeps its bytes and its mode.
pub(crate) fn open_private(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
