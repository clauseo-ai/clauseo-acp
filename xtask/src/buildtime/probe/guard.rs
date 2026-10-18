//! The guard over one marker file: the saved copy of its original, every write the probe makes, and
//! putting the original back. The guard never writes over bytes it did not write itself.

use super::super::marker::Marker;
use super::super::verdict::Failure;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Where the copies of the original files wait, under the repo root.
pub(super) const BACKUP_DIR: &str = "target/xtask/buildtime";

/// A marker file's original text, the copy of it under `target/xtask/buildtime`, and the bytes the
/// guard last wrote to the file (the original until the first write).
///
/// Every write, and putting the original back, first reads the file and compares it with those
/// bytes. When they differ, someone edited the file during the run: nothing more is written, the
/// copy is kept, and the failure names both. `restore` puts the original back and deletes the copy.
/// A guard dropped before that does the same, under the same rule.
pub(super) struct Guard {
    path: PathBuf,
    rel: String,
    original: String,
    pub(super) last: String,
    base: PathBuf,
    backup: PathBuf,
    /// The original still has to go back. False after `restore`, and after a conflict, which keeps
    /// the file and the copy as they are.
    pending: bool,
}

impl Guard {
    /// Saves the original of `marker` as its copy. Nothing is edited when the save fails, and an
    /// existing copy is never replaced.
    pub(super) fn save(root: &Path, marker: &Marker) -> Result<Guard, Failure> {
        let (rel, text) = (marker.path.as_str(), marker.text.as_str());
        let base = root.join(BACKUP_DIR);
        let backup = base.join(rel);
        match write_new_copy(&backup, text) {
            Ok(true) => Ok(Guard {
                path: root.join(rel),
                rel: rel.to_owned(),
                original: text.to_owned(),
                last: text.to_owned(),
                base,
                backup,
                pending: true,
            }),
            Ok(false) => Err(Failure::new(
                format!(
                    "a saved copy of {rel} already exists at {}; an earlier restore failed",
                    copy_name(rel)
                ),
                format!("restore {rel} from that copy by hand, then delete it"),
            )),
            Err(error) => Err(Failure::new(
                format!("could not back up {rel} before editing it: {error}"),
                save_fix(error.kind()),
            )),
        }
    }

    /// Writes `text` over the file, once the file still holds the bytes last written.
    pub(super) fn write(&mut self, text: String) -> Result<(), Failure> {
        self.unchanged()?;
        fs::write(&self.path, text.as_bytes()).map_err(|error| {
            Failure::new(
                format!("could not write {}: {error}", self.rel),
                format!(
                    "make sure the file is writable; its original is in {}",
                    copy_name(&self.rel)
                ),
            )
        })?;
        self.last = text;
        Ok(())
    }

    /// `Ok` when the file still holds the bytes last written. Otherwise the guard stops: it keeps
    /// the file and the copy, and the failure says so.
    fn unchanged(&mut self) -> Result<(), Failure> {
        let found = match fs::read(&self.path) {
            Ok(bytes) if bytes == self.last.as_bytes() => return Ok(()),
            Ok(_) => conflict(&self.rel, "changed after a running buildtime run"),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                conflict(&self.rel, "is missing after a running buildtime run")
            }
            Err(error) => Failure::new(
                format!("could not read {}: {error}", self.rel),
                format!(
                    "compare it with {}, keep what you want, then delete the copy",
                    copy_name(&self.rel)
                ),
            ),
        };
        self.pending = false;
        Err(found)
    }

    /// The original text back in the file, then the copy deleted, once the file still holds the
    /// bytes last written.
    fn put_back(&mut self) -> Result<(), Failure> {
        self.unchanged()?;
        self.pending = false;
        fs::write(&self.path, self.original.as_bytes()).map_err(|error| {
            Failure::new(
                format!("could not restore {}: {error}", self.rel),
                format!(
                    "copy {} over {}; it holds the original text",
                    self.backup.display(),
                    self.rel
                ),
            )
        })?;
        fs::remove_file(&self.backup).map_err(|error| {
            Failure::new(
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
    pub(super) fn restore(mut self) -> Result<(), Failure> {
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

/// The fix for a copy that could not be saved. A filesystem without hard links needs another
/// filesystem, not write access.
pub(super) fn save_fix(kind: io::ErrorKind) -> String {
    if kind == io::ErrorKind::Unsupported {
        format!(
            "the filesystem of {BACKUP_DIR} has no hard links; run from a checkout on a filesystem that has them"
        )
    } else {
        format!("make sure {BACKUP_DIR} is writable, then run again")
    }
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

/// The copy's path under the repo root, as the messages print it.
pub(super) fn copy_name(rel: &str) -> String {
    format!("{BACKUP_DIR}/{rel}")
}

/// Removes the directories between `file` and `base` that are now empty.
pub(super) fn prune(base: &Path, file: &Path) {
    let mut dir = file.parent();
    while let Some(current) = dir.filter(|d| d.starts_with(base) && *d != base) {
        if fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
}

/// A conflict: the file and its saved copy are kept, and the fix says what to do with them.
pub(super) fn conflict(rel: &str, what: &str) -> Failure {
    Failure::new(
        format!("{rel} {what}; its saved copy is {}", copy_name(rel)),
        format!(
            "compare the two, keep what you want, then delete {}",
            copy_name(rel)
        ),
    )
}
