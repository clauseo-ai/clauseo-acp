//! The std calls that change files, for the tests alone: making and removing each test's
//! folder, and writing the damaged or foreign files the open tests read. Workspace
//! `clippy.toml` bans them everywhere else; the expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the search tests make their folders and write damaged files here alone"
)]

use std::path::Path;

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
}

/// `fs::remove_dir_all`, ignoring a folder that is already gone.
pub(crate) fn remove_dir_all(path: &Path) {
    match std::fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("removing {}: {error}", path.display()),
    }
}

/// `fs::write`.
pub(crate) fn write(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
}
