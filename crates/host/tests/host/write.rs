//! The std calls that change files, for the tests alone: making and removing each test's
//! folder, writing its `settings.json`, seeding matter folders and the lawyer's own edits.
//! Workspace `clippy.toml` bans them everywhere else; the expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the host's tests make their folders and write their fixture files here alone"
)]

use std::path::Path;

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
}

/// `fs::remove_dir_all`, ignoring a folder that is already gone. Returns the error instead of
/// panicking, so a `Drop` can call it while a test unwinds.
pub(crate) fn remove_dir_all(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// `fs::write`, creating the parent folder first.
pub(crate) fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}
