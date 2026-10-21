//! What a tool tells the guard about itself, and the guard's recovery of what an interrupted run
//! left: its temporary files and its copies.

use super::super::replace::{replace, tmp_of};
use super::super::sys;
use super::{GuardFailure, is_repo_path, prune, refusal, tmp_rel};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// What a tool that edits tracked files tells the guard: where its copies wait and its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Backups {
    /// The folder of the copies, under the repo root, `/`-separated: `target/xtask/buildtime`.
    /// A copy keeps the file's repo path under it.
    pub dir: &'static str,
    /// The tool, as its temporary files (`.<name>.<tool>.tmp`) and messages ("this buildtime
    /// run") name it.
    pub tool: &'static str,
}

impl Backups {
    /// One run of the tool, as the messages name it: `buildtime run`.
    pub(crate) fn run(self) -> String {
        format!("{} run", self.tool)
    }

    /// The copy's path under the repo root, as the messages print it.
    pub(crate) fn copy_name(self, rel: &str) -> String {
        format!("{}/{rel}", self.dir)
    }

    /// The folder of the copies under `root`.
    pub(crate) fn base(self, root: &Path) -> PathBuf {
        root.join(self.dir)
    }

    /// Whether `rel`, a path under the backup folder, names one of the guard's temporary files
    /// (`.<name>.<tool>.tmp`) rather than a copy.
    pub(crate) fn is_tmp(self, rel: &str) -> bool {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        name.starts_with('.') && name.ends_with(&format!(".{}.tmp", self.tool))
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

    /// Deletes the guard's temporary file at `rel` under the backup folder, which an interrupted
    /// save left. Refuses any other path: one outside the backup folder, or not named
    /// `.<name>.<tool>.tmp`.
    pub(crate) fn discard_interrupted_save(
        self,
        root: &Path,
        rel: &str,
    ) -> Result<(), GuardFailure> {
        if !is_repo_path(rel) || !self.is_tmp(rel) {
            return Err(GuardFailure::new(
                format!(
                    "refusing to delete `{rel}` under {}: it is not a temporary file of the guard (.<name>.{}.tmp)",
                    self.dir, self.tool
                ),
                "pass the temporary file's path under the backup folder".to_owned(),
            ));
        }
        let copy = self.copy_name(rel);
        sys::remove_file(&self.base(root).join(rel)).map_err(|error| {
            GuardFailure::new(
                format!("could not delete {copy}: {error}"),
                format!("delete {copy} by hand: an interrupted save left it, and the next save needs its name"),
            )
        })
    }

    /// Deletes the temporary file an interrupted write to the file at repo path `rel` left next to
    /// it, when there is one. It only ever holds bytes the tool wrote.
    pub(crate) fn discard_interrupted_write(
        self,
        root: &Path,
        rel: &str,
    ) -> Result<(), GuardFailure> {
        if !is_repo_path(rel) {
            return Err(refusal(rel));
        }
        let tmp = tmp_of(&root.join(rel), self.tool);
        match sys::remove_file(&tmp) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(GuardFailure::new(
                format!("could not delete {}: {error}", tmp_rel(rel, self.tool)),
                format!(
                    "delete {} by hand: an interrupted {} wrote it, and it holds only that run's edit",
                    tmp_rel(rel, self.tool),
                    self.run()
                ),
            )),
            _ => Ok(()),
        }
    }

    /// Puts the copy at `<dir>/<rel>`, which an interrupted run left, back over the file at repo
    /// path `rel` (through a temporary file renamed over it), then deletes the copy and the
    /// folders it leaves empty. Call it only once the tool has checked that the file is the copy
    /// plus an edit the tool itself makes. Refuses a `rel` that is not a relative path inside
    /// the repo, or that names a temporary file.
    pub(crate) fn restore_leftover(self, root: &Path, rel: &str) -> Result<(), GuardFailure> {
        if !is_repo_path(rel) || self.is_tmp(rel) {
            return Err(refusal(rel));
        }
        let copy = self.copy_name(rel);
        let backup = self.base(root).join(rel);
        let original = fs::read(&backup).map_err(|error| {
            GuardFailure::new(
                format!("could not read {copy}: {error}; {rel} is unchanged"),
                "fix what the error names, then run again".to_owned(),
            )
        })?;
        replace(&root.join(rel), self.tool, &original).map_err(|failure| {
            let left = match failure.tmp_left {
                Some(_) => format!(
                    "; delete {}: {copy} holds the same text",
                    tmp_rel(rel, self.tool)
                ),
                None => String::new(),
            };
            GuardFailure::new(
                format!(
                    "could not restore {rel} from its copy {copy}: {}; {rel} is unchanged",
                    failure.error
                ),
                format!("fix what the error names, then run again{left}"),
            )
        })?;
        sys::remove_file(&backup).map_err(|error| {
            GuardFailure::new(
                format!("restored {rel} but could not delete its copy {copy}: {error}"),
                format!("delete {copy}; the file is already restored"),
            )
        })?;
        prune(&self.base(root), &backup);
        Ok(())
    }
}
