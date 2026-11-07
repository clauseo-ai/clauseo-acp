//! The exes tier B needs beside the app's own exe, and whether they are there.
//!
//! Windows security can quarantine `clauseo-sandbox-setup.exe` (Defender has flagged it as a
//! false positive), right after install or after it runs. Without it, tier B can't top up its
//! pool, reclaim what a sandbox user took, or be removed, so tier B must not run: the launch
//! check refuses ([`check_launch`](super::clean_close::check_launch)), and starting the exe
//! fails, with [`Blocked::ToolMissing`] either way, never a weaker tier in silence.
//!
//! An exe counts as there only as a regular file: a link, a junction or any other reparse point
//! in its place counts as missing, since it could point anywhere.

use std::path::{Path, PathBuf};

use crate::{Blocked, SetupError};

/// The elevated set-up program, bundled beside `Clauseo.exe` (`UserSetup` starts it).
pub const SETUP_EXE: &str = "clauseo-sandbox-setup.exe";

/// Every exe a tier-B launch or its clean-up needs beside the app's exe: the set-up program,
/// for top-up, `--reclaim` and `--remove`.
pub const BUNDLED_EXES: &[&str] = &[SETUP_EXE];

/// The folder the running exe is in, where the bundled exes live; `None` when Windows can't
/// say.
pub fn bundle_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.parent().map(Path::to_path_buf)
}

/// Whether `exe` is a regular file, not a link or other reparse point. Nothing is followed.
pub fn present(exe: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(exe) else {
        return false;
    };
    meta.file_type().is_file() && !reparse_point(&meta)
}

#[cfg(windows)]
fn reparse_point(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    /// `FILE_ATTRIBUTE_REPARSE_POINT`.
    const REPARSE_POINT: u32 = 0x400;
    meta.file_attributes() & REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn reparse_point(meta: &std::fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

/// Every one of [`BUNDLED_EXES`] in `dir`, or [`Blocked::ToolMissing`]. A missing exe's name
/// is logged; its folder is not.
pub fn check_bundled(dir: &Path) -> Result<(), Blocked> {
    for name in BUNDLED_EXES {
        if !present(&dir.join(name)) {
            tracing::warn!(
                exe = name,
                "a bundled sandbox exe is missing beside the app"
            );
            return Err(Blocked::ToolMissing);
        }
    }
    Ok(())
}

/// [`check_bundled`] beside the running exe; a running exe Windows can't name is missing too.
pub fn check_bundled_here() -> Result<(), Blocked> {
    let Some(dir) = bundle_dir() else {
        tracing::warn!("the running exe's folder is unknown, so its bundled exes are too");
        return Err(Blocked::ToolMissing);
    };
    check_bundled(&dir)
}

/// Starts `exe` through `start` once it is [`present`], else [`Blocked::ToolMissing`] without
/// calling `start`. `start` still maps a file gone by the time it runs (`launch_error`).
pub fn start_present<T>(
    exe: &Path,
    start: impl FnOnce() -> Result<T, SetupError>,
) -> Result<T, SetupError> {
    if !present(exe) {
        tracing::warn!("the set-up program is missing; Windows security may have removed it");
        return Err(SetupError::Blocked(Blocked::ToolMissing));
    }
    start()
}

/// A folder `app` in `dir` holding a stand-in for each of [`BUNDLED_EXES`]: the bundle the
/// launch check's tests look in (`check_launch_beside`).
#[cfg(test)]
pub(crate) fn fake_bundle(dir: &crate::test_support::TempDir) -> PathBuf {
    for name in BUNDLED_EXES {
        dir.file(&format!("app/{name}"), "MZ");
    }
    dir.path().join("app")
}

#[cfg(test)]
mod tests;
