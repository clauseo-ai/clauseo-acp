//! The tracked-file guard: the saved copy of a file's original, every write a tool makes to the
//! file, and putting the original back. The guard never writes over bytes it did not write itself.
//! Any xtask tool that edits a tracked file uses it; `buildtime` does for its marker files.
//!
//! Its rules, the same as `ui/app/scripts/guard.mjs`'s:
//! - Before the first edit, the original is copied to `<dir>/<repo path>`. A copy is never
//!   replaced.
//! - Every write to the file goes to `.<name>.<tool>.tmp` in the same folder, then is renamed over
//!   it, so a kill never leaves half a file.
//! - The original goes back only while the file holds exactly the bytes last written. Anything else
//!   is the developer's edit: nothing more is written, and both files stay.
//! - A killed run leaves its copy. The tool's next run puts it back through
//!   `Backups::restore_leftover` only when the tool has checked that the file is the copy plus an
//!   edit the tool itself makes.

mod backups;

pub(crate) use backups::Backups;

use super::replace::{ReplaceFailure, replace, tmp_of};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

/// Why the guard stopped or refused: what happened, and what to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuardFailure {
    pub message: String,
    pub fix: String,
}

impl GuardFailure {
    fn new(message: String, fix: String) -> GuardFailure {
        GuardFailure { message, fix }
    }
}

/// A file's original text, the copy of it under the tool's backup folder, and the bytes the
/// guard last wrote to the file (the original until the first write).
///
/// Every write, and putting the original back, first reads the file and compares it with those
/// bytes. When they differ, someone edited the file during the run: nothing more is written, the
/// copy is kept, and the failure names both. `restore` puts the original back and deletes the copy.
/// A guard dropped before that does the same, under the same rule.
pub(crate) struct Guard {
    backups: Backups,
    path: PathBuf,
    rel: String,
    original: String,
    pub(crate) last: String,
    base: PathBuf,
    backup: PathBuf,
    /// The original still has to go back. False after `restore`, and after a conflict, which keeps
    /// the file and the copy as they are.
    pending: bool,
}

impl Guard {
    /// Saves `text`, the original of the file at repo path `rel` under `root`, as its copy.
    /// Nothing is edited when the save fails, and an existing copy is never replaced. Refuses a
    /// `rel` that is not a relative path inside the repo.
    pub(crate) fn save(
        root: &Path,
        backups: Backups,
        rel: &str,
        text: &str,
    ) -> Result<Guard, GuardFailure> {
        if !is_repo_path(rel) {
            return Err(refusal(rel));
        }
        let base = backups.base(root);
        let backup = base.join(rel);
        let copy = backups.copy_name(rel);
        let tmp = format!("{}/{}", backups.dir, tmp_rel(rel, backups.tool));
        match write_new_copy(&backup, backups.tool, text, |from, to| {
            fs::hard_link(from, to)
        }) {
            Ok(()) => Ok(Guard {
                backups,
                path: root.join(rel),
                rel: rel.to_owned(),
                original: text.to_owned(),
                last: text.to_owned(),
                base,
                backup,
                pending: true,
            }),
            Err(Save::CopyExists) => Err(GuardFailure::new(
                format!(
                    "a saved copy of {rel} already exists at {copy}; an earlier {} kept it (a conflict or a failed restore)",
                    backups.run()
                ),
                format!("compare {rel} with {copy}, keep what you want, then delete {copy}"),
            )),
            Err(Save::TmpExists) => Err(GuardFailure::new(
                format!("could not back up {rel}: {tmp} is left from an interrupted save"),
                format!("compare {tmp} with {rel}, keep what you want, then delete {tmp}"),
            )),
            Err(Save::Failed { error, tmp_left }) => {
                let left = if tmp_left {
                    format!("; delete {tmp}, a partial copy")
                } else {
                    String::new()
                };
                let fix = if error.kind() == io::ErrorKind::Unsupported {
                    format!(
                        "the filesystem of {} has no hard links; run from a checkout on a filesystem that has them",
                        backups.dir
                    )
                } else {
                    format!("fix what the error names, then run again; {rel} was not edited")
                };
                Err(GuardFailure::new(
                    format!("could not back up {rel} before editing it: {error}"),
                    format!("{fix}{left}"),
                ))
            }
        }
    }

    /// Writes `text` over the file, once the file still holds the bytes last written.
    pub(crate) fn write(&mut self, text: String) -> Result<(), GuardFailure> {
        self.unchanged()?;
        replace(&self.path, self.backups.tool, text.as_bytes()).map_err(|failure| {
            let left = self.tmp_left(&failure, "it holds only this run's edit");
            GuardFailure::new(
                format!(
                    "could not write {}: {}; the file is unchanged",
                    self.rel, failure.error
                ),
                format!("fix what the error names, then run again{left}"),
            )
        })?;
        self.last = text;
        Ok(())
    }

    /// `; delete <tmp>: <why>` when `failure` left the file's temporary file, else nothing.
    fn tmp_left(&self, failure: &ReplaceFailure, why: &str) -> String {
        match failure.tmp_left {
            Some(_) => format!("; delete {}: {why}", tmp_rel(&self.rel, self.backups.tool)),
            None => String::new(),
        }
    }

    /// `Ok` when the file still holds the bytes last written. Otherwise the guard stops: it keeps
    /// the file and the copy, and the failure says so.
    fn unchanged(&mut self) -> Result<(), GuardFailure> {
        let run = self.backups.run();
        let found = match fs::read(&self.path) {
            Ok(bytes) if bytes == self.last.as_bytes() => return Ok(()),
            Ok(_) => self
                .backups
                .conflict(&self.rel, &format!("changed during this {run}")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => self
                .backups
                .conflict(&self.rel, &format!("went missing during this {run}")),
            Err(error) => GuardFailure::new(
                format!("could not read {}: {error}", self.rel),
                format!(
                    "compare it with {}, keep what you want, then delete the copy",
                    self.backups.copy_name(&self.rel)
                ),
            ),
        };
        self.pending = false;
        Err(found)
    }

    /// The original text back in the file, then the copy deleted, once the file still holds the
    /// bytes last written.
    fn put_back(&mut self) -> Result<(), GuardFailure> {
        self.unchanged()?;
        self.pending = false;
        let copy = self.backups.copy_name(&self.rel);
        replace(&self.path, self.backups.tool, self.original.as_bytes()).map_err(|failure| {
            let left = self.tmp_left(&failure, "the saved copy holds the original");
            GuardFailure::new(
                format!(
                    "could not restore {}: {}; it still holds this run's edit, and its original is in {copy}",
                    self.rel, failure.error
                ),
                format!(
                    "fix what the error names, then copy {copy} over {}: the file holds nothing \
                     but this run's edit{left}",
                    self.rel
                ),
            )
        })?;
        fs::remove_file(&self.backup).map_err(|error| {
            GuardFailure::new(
                format!(
                    "restored {} but could not delete its copy {copy}: {error}",
                    self.rel
                ),
                format!("delete {copy}; the file is already restored"),
            )
        })?;
        prune(&self.base, &self.backup);
        Ok(())
    }

    /// Puts the original back and deletes the copy. After a conflict, which was already returned
    /// by the write that found it, the file and the copy stay as they are.
    pub(crate) fn restore(mut self) -> Result<(), GuardFailure> {
        if self.pending {
            self.put_back()
        } else {
            Ok(())
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        // A panic or an early return lands here. The file goes back only when it still holds the
        // bytes last written; otherwise both stay. There is no way to print from a drop, so the
        // outcome is not kept, and a failure keeps the copy for the next run.
        if self.pending {
            let _restored = self.put_back().is_ok();
        }
    }
}

/// Why `write_new_copy` wrote no copy.
enum Save {
    /// The copy already exists; nothing was written.
    CopyExists,
    /// The temporary file of an earlier, interrupted save is in the way; nothing was written.
    TmpExists,
    /// The OS refused a step. `tmp_left` is true when the temporary file could not be deleted.
    Failed { error: io::Error, tmp_left: bool },
}

/// Writes `text` to `backup` without ever replacing a file there. The bytes go to the temporary
/// file `tmp_of(backup, tool)`, created only when it is missing, which is then hard-linked to
/// `backup` by `link` (`fs::hard_link` in `save`): a link fails when `backup` exists, so the check
/// and the create are one step. The copy is synced before it is linked, and the temporary file is
/// deleted again. `link` is a parameter so a test can make it fail the way the OS can.
fn write_new_copy(
    backup: &Path,
    tool: &str,
    text: &str,
    link: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<(), Save> {
    let failed = |error| Save::Failed {
        error,
        tmp_left: false,
    };
    if let Some(dir) = backup.parent() {
        fs::create_dir_all(dir).map_err(failed)?;
    }
    let tmp = tmp_of(backup, tool);
    let mut file = match OpenOptions::new().write(true).create_new(true).open(&tmp) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Err(Save::TmpExists),
        Err(error) => return Err(failed(error)),
    };
    let linked = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| link(&tmp, backup));
    drop(file);
    let tmp_left = fs::remove_file(&tmp).is_err();
    match linked {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(Save::CopyExists),
        Err(error) => Err(Save::Failed { error, tmp_left }),
    }
}

/// Whether `rel` is a relative path inside the repo: one or more plain names, no `..`, no root.
fn is_repo_path(rel: &str) -> bool {
    let path = Path::new(rel);
    !rel.is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// The refusal of a path that is not a relative path inside the repo.
fn refusal(rel: &str) -> GuardFailure {
    GuardFailure::new(
        format!("refusing to guard `{rel}`: it is not a relative path inside the repo"),
        "pass the file's repo path, such as crates/desktop/src/lib.rs".to_owned(),
    )
}

/// The temporary file of a write by `tool` to the repo path `rel`, as a repo path.
fn tmp_rel(rel: &str, tool: &str) -> String {
    match rel.rsplit_once('/') {
        Some((dir, name)) => format!("{dir}/.{name}.{tool}.tmp"),
        None => format!(".{rel}.{tool}.tmp"),
    }
}

/// Removes the directories between `file` and `base` that are now empty.
fn prune(base: &Path, file: &Path) {
    let mut dir = file.parent();
    while let Some(current) = dir.filter(|d| d.starts_with(base) && *d != base) {
        if fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
}

#[cfg(test)]
mod tests;
