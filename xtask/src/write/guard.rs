//! The tracked-file guard: the saved copy of a file's original, every write a tool makes to the
//! file, and putting the original back. The guard never writes over bytes it did not write itself.
//! Any xtask tool that edits a tracked file uses it; `buildtime` does for its marker files.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// What a tool that edits tracked files tells the guard: where its copies wait and what its
/// messages call one run of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Backups {
    /// The folder of the copies, under the repo root, `/`-separated: `target/xtask/buildtime`.
    /// A copy keeps the file's repo path under it.
    pub dir: &'static str,
    /// One run of the tool, as in "changed during this buildtime run".
    pub run: &'static str,
}

/// Why the guard stopped: what happened, and what to do about it.
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

impl Backups {
    /// The copy's path under the repo root, as the messages print it.
    pub(crate) fn copy_name(self, rel: &str) -> String {
        format!("{}/{rel}", self.dir)
    }

    /// The folder of the copies under `root`.
    pub(crate) fn base(self, root: &Path) -> PathBuf {
        root.join(self.dir)
    }

    /// The fix for a copy that could not be saved. A filesystem without hard links needs another
    /// filesystem, not write access.
    pub(crate) fn save_fix(self, kind: io::ErrorKind) -> String {
        if kind == io::ErrorKind::Unsupported {
            format!(
                "the filesystem of {} has no hard links; run from a checkout on a filesystem that has them",
                self.dir
            )
        } else {
            format!("make sure {} is writable, then run again", self.dir)
        }
    }

    /// A conflict: the file and its saved copy are kept, and the fix says what to do with them.
    pub(crate) fn conflict(self, rel: &str, what: &str) -> GuardFailure {
        GuardFailure::new(
            format!("{rel} {what}; its saved copy is {}", self.copy_name(rel)),
            format!(
                "compare the two, keep what you want, then delete {}",
                self.copy_name(rel)
            ),
        )
    }

    /// Puts `original`, the bytes of the copy `copy` that an interrupted run left, back in the
    /// file at `rel` under `root`, then deletes the copy and the folders it leaves empty. The
    /// caller has already checked that the file differs from the copy only by the tool's edit.
    pub(crate) fn restore_leftover(
        self,
        root: &Path,
        rel: &str,
        copy: &Path,
        original: &[u8],
    ) -> io::Result<()> {
        fs::write(root.join(rel), original).and_then(|()| fs::remove_file(copy))?;
        prune(&self.base(root), copy);
        Ok(())
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
    /// Saves `text`, the original of the file at `rel` under `root`, as its copy. Nothing is
    /// edited when the save fails, and an existing copy is never replaced.
    pub(crate) fn save(
        root: &Path,
        backups: Backups,
        rel: &str,
        text: &str,
    ) -> Result<Guard, GuardFailure> {
        let base = backups.base(root);
        let backup = base.join(rel);
        let copy = backups.copy_name(rel);
        match write_new_copy(&backup, text) {
            Ok(true) => Ok(Guard {
                backups,
                path: root.join(rel),
                rel: rel.to_owned(),
                original: text.to_owned(),
                last: text.to_owned(),
                base,
                backup,
                pending: true,
            }),
            Ok(false) => Err(GuardFailure::new(
                format!(
                    "a saved copy of {rel} already exists at {copy}; an earlier {} kept it (a conflict or a failed restore)",
                    backups.run
                ),
                format!("compare {rel} with {copy}, keep what you want, then delete {copy}"),
            )),
            Err(error) => Err(GuardFailure::new(
                format!("could not back up {rel} before editing it: {error}"),
                backups.save_fix(error.kind()),
            )),
        }
    }

    /// Writes `text` over the file, once the file still holds the bytes last written.
    pub(crate) fn write(&mut self, text: String) -> Result<(), GuardFailure> {
        self.unchanged()?;
        fs::write(&self.path, text.as_bytes()).map_err(|error| {
            GuardFailure::new(
                format!("could not write {}: {error}", self.rel),
                format!(
                    "make sure the file is writable; its original is in {}",
                    self.backups.copy_name(&self.rel)
                ),
            )
        })?;
        self.last = text;
        Ok(())
    }

    /// `Ok` when the file still holds the bytes last written. Otherwise the guard stops: it keeps
    /// the file and the copy, and the failure says so.
    fn unchanged(&mut self) -> Result<(), GuardFailure> {
        let run = self.backups.run;
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
        fs::write(&self.path, self.original.as_bytes()).map_err(|error| {
            GuardFailure::new(
                format!("could not restore {}: {error}", self.rel),
                format!(
                    "copy {} over {}; it holds the original text",
                    self.backup.display(),
                    self.rel
                ),
            )
        })?;
        fs::remove_file(&self.backup).map_err(|error| {
            GuardFailure::new(
                format!(
                    "restored {} but could not delete its copy {}: {error}",
                    self.rel,
                    self.backup.display()
                ),
                format!(
                    "delete {}; the file is already restored",
                    self.backup.display()
                ),
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

/// Deletes `tmp`, the temporary `<copy>.tmp` file that an interrupted save (`Guard::save`) left
/// among the copies.
pub(crate) fn discard_interrupted_save(tmp: &Path) -> io::Result<()> {
    fs::remove_file(tmp)
}

/// Writes `text` to `backup` without ever replacing a file there. The bytes go to `<backup>.tmp`,
/// which is then hard-linked to `backup`: a link fails when `backup` exists, so the check and the
/// create are one step. Returns `false` when `backup` already existed. The copy is synced before it
/// is linked. The temporary file is removed again; a failure to remove it is ignored, since the
/// next run's start deletes it.
fn write_new_copy(backup: &Path, text: &str) -> io::Result<bool> {
    if let Some(dir) = backup.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp_name = backup.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    let linked = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::hard_link(&tmp, backup));
    let _tmp_removed = fs::remove_file(&tmp).is_ok();
    match linked {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
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
