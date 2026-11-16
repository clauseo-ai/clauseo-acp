//! Windows: `long_name`'s Win32 calls, made by the chooser's FFI module
//! (`chooser/windows.rs`: `GetLongPathNameW`, and for the tests `GetShortPathNameW`).

use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use super::with_room;

/// `path` spelt out by `GetLongPathNameW`; `None` when it does not exist or can't be read.
pub(super) fn os_long_name(path: &Path) -> Option<PathBuf> {
    let long = with_room(|room| crate::chooser::long_path_name(path, room))?;
    Some(std::ffi::OsString::from_wide(&long).into())
}

/// Tests: `path`'s 8.3 form by `GetShortPathNameW`; `None` when it can't be read.
#[cfg(test)]
pub(super) fn os_short_name(path: &Path) -> Option<PathBuf> {
    let short = with_room(|room| crate::chooser::short_path_name(path, room))?;
    Some(std::ffi::OsString::from_wide(&short).into())
}
