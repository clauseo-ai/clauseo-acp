//! Whether the matter folder may get its read allow rules (`own_allow.rs`
//! `matter_shell_allow_rules`): not when a link in it leads outside it. Claude Code 2.1.293's
//! path check reads only the folder before a glob (`cat <matter>/*/x`, `ls <matter>/li?k/`),
//! so through such a link a glob would read what the link leads to, `<data>` or another matter,
//! with no classifier (the security review of #1065, H2). The check here walks the matter once
//! at the launch, never following a link, and finds every symbolic link, junction and folder
//! symbolic link (std's `is_symlink`, a name-surrogate reparse point on Windows) whose target,
//! resolved, is not in the matter; a broken one counts as outside. A walk past [`SCAN_CAP`]
//! entries, or a matter that can't be listed, gives no rules too: the classifier judges, as
//! before. A sub-folder that can't be listed is skipped, as `clauseo_sandbox::scan_matter` does:
//! the agent runs as the same user and can't list it either.
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

#[cfg(test)]
#[path = "matter_links_tests.rs"]
mod tests;
