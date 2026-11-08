//! What the desktop compares at start-up before it asks for a refresh
//! ([`super::request_admin_refresh`]): the record's `AdminCopySha256`, and the SHA-256 of
//! `clauseo-sandbox-setup.exe` in the record's `UserInstallDir`, the folder the refresh task
//! copies from. The lawyer reads both, unelevated. Neither is trusted: a wrong answer only
//! queues a run of the task, which checks everything itself and ends at once with nothing new.

use std::io;

use crate::windows::admin_copy::{SETUP_EXE, os};
use crate::windows::record::{RecordError, admin_copy_sha256, user_install_dir};

/// The record's `AdminCopySha256`, unchecked text; `Ok(None)` when the record holds none (tier
/// B's admin copy isn't placed).
pub fn recorded_admin_copy_sha256() -> Result<Option<String>, RecordError> {
    admin_copy_sha256()
}

/// The SHA-256 (64 lowercase hex digits) of `clauseo-sandbox-setup.exe` in the record's
/// `UserInstallDir`, hashed as the admin copy's check hashes it (`admin_copy::os::sha256`: one
/// handle, a reparse point refused). `Ok(None)` when the record names no install folder; an
/// error when the file is missing or can't be read.
pub fn installed_setup_sha256() -> io::Result<Option<String>> {
    let Some(dir) = user_install_dir().map_err(io::Error::other)? else {
        return Ok(None);
    };
    os::sha256(&dir.join(SETUP_EXE)).map(Some)
}
