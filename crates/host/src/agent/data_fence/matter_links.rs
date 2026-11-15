//! Whether the matter folder may get its read allow rules (`own_allow.rs`
//! `matter_shell_allow_rules`): not when a link in it leads outside it. Claude Code 2.1.293's
//! path check reads only the folder before a glob (`cat <matter>/*/x`, `ls <matter>/li?k/`),
//! so through such a link a glob would read what the link leads to, `<data>` or another matter,
//! with no classifier (the security review of #1065, H2). The check here walks the matter once
//! at the launch, never following a link, and finds every symbolic link, junction and folder
//! symbolic link (std's `is_symlink`, a name-surrogate reparse point on Windows) whose target,
//! resolved, is not in the matter; a broken one counts as outside. On Windows any other reparse
//! point counts as a link out too, unless its tag is a known local kind of data
//! (`network_paths::local_reparse_tag`: `OneDrive`'s cloud files, dedup, WOF, WCI, an app
//! execution alias, `ProjFS`, an `AF_UNIX` socket; `counts_as_link`), so a matter in `OneDrive`
//! keeps its rules while one holding a DFS or Azure File Sync point does not. A `.lnk` shortcut
//! is a plain file to the file system and to Claude Code's tools: not a link. A walk past
//! [`SCAN_CAP`] entries, or a matter that can't be listed, gives no rules too: the classifier
//! judges, as before. A sub-folder that can't be listed is skipped, as
//! `clauseo_sandbox::scan_matter` does: the agent runs as the same user and can't list it either.
//!
//! The residual: a link made after the launch. Making one is a write (`ln -s`, `mklink /J`,
//! `New-Item -ItemType Junction`) that no allow rule matches, so it needs a classifier's pass
//! first; on macOS the sandbox's `denyRead` of `<data>` and the other matters still holds behind
//! it. A hard link to a file outside is not found here, and the Read tool reads one the same way.

use std::path::{Path, PathBuf};

/// The most entries the walk lists before it gives up (and the matter gets no rules).
pub(crate) const SCAN_CAP: usize = 50_000;

/// The matter's spellings that may get read allow rules: `real` (the folder Claude works in)
/// and `stored` when it differs, or none when the walk of `real` finds a link out of it, passes
/// [`SCAN_CAP`] or can't list it. Reads the file system: on the launch-prep thread.
pub(crate) fn readable_matter(real: &Path, stored: &Path) -> Vec<PathBuf> {
    match links_out(real, SCAN_CAP) {
        Some(false) => std::iter::once(real.to_owned())
            .chain((real != stored).then(|| stored.to_owned()))
            .collect(),
        found => {
            let reason = if found.is_some() {
                "a link leads out of it"
            } else {
                "it could not be walked within the cap"
            };
            tracing::info!(reason, "the matter's read allow rules were left out");
            Vec::new()
        }
    }
}

/// Whether a link in `matter` leads outside it (`Some(true)`), none does (`Some(false)`), or
/// the walk could not tell: `matter` can't be resolved or listed, or it holds more than `cap`
/// entries (`None`).
pub(crate) fn links_out(matter: &Path, cap: usize) -> Option<bool> {
    let root = dunce::canonicalize(matter).ok()?;
    let mut folders = vec![root.clone()];
    let mut seen = 0_usize;
    let mut first = true;
    while let Some(folder) = folders.pop() {
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(_) if first => return None,
            Err(error) => {
                tracing::debug!(%error, "the matter's link walk skipped a folder");
                continue;
            }
        };
        first = false;
        for entry in entries.flatten() {
            seen += 1;
            if seen > cap {
                return None;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if !kind.is_symlink() && unknown_reparse_point(&entry, &path) {
                return Some(true);
            }
            if kind.is_symlink() {
                let inside =
                    dunce::canonicalize(&path).is_ok_and(|target| target.starts_with(&root));
                if !inside {
                    return Some(true);
                }
            } else if kind.is_dir() {
                folders.push(path);
            }
        }
    }
    Some(false)
}

/// Whether a reparse point with `tag` counts as a link that may lead out of the matter: a
/// symbolic link, a junction or mount point, or any kind not known to be local data. Pure, so
/// it is tested on every system.
#[cfg(any(windows, test))]
fn counts_as_link(tag: u32) -> bool {
    !crate::network_paths::local_reparse_tag(tag)
}

/// Whether `entry` (at `path`, not a name-surrogate link) is a reparse point that
/// [`counts_as_link`]; one whose tag can't be read counts too. Never on other systems.
#[cfg(windows)]
fn unknown_reparse_point(entry: &std::fs::DirEntry, path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    /// `FILE_ATTRIBUTE_REPARSE_POINT`.
    const REPARSE_POINT: u32 = 0x400;
    let Ok(meta) = entry.metadata() else {
        return true;
    };
    meta.file_attributes() & REPARSE_POINT != 0
        && crate::network_paths::reparse_tag(path).is_none_or(counts_as_link)
}

/// Never on systems without reparse points.
#[cfg(not(windows))]
fn unknown_reparse_point(_entry: &std::fs::DirEntry, _path: &Path) -> bool {
    false
}

#[cfg(test)]
#[path = "matter_links_tests.rs"]
mod tests;
