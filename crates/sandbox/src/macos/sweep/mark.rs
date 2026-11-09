//! [`SessionMark`]: a launch's mark and canary, the two files the session sweep asks the kernel
//! about. The one place the sweep changes files.

use std::ffi::{CStr, CString};
use std::fs::{DirBuilder, OpenOptions};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use super::{ProcId, SweepError, now_wall, start_of};
use crate::macos::profile::{SESSION_MARK, check_tag};

/// The mark's file name in its folder.
const MARK: &str = "mark";
/// The canary's file name, beside the mark.
const CANARY: &str = "canary";

/// A launch's session mark: `<dir>/<tag>/mark`, which only this launch's profile lets its
/// processes write (`file-write-data`, nothing else), and `<dir>/<tag>/canary` beside it, which
/// no profile names. Both are regular files of this user, mode 0600, made before the launch: the
/// kernel answers "denied" for a path that does not exist, for every process.
///
/// `dir` must sit outside every folder any profile lets an agent write (the launcher checks it
/// against its policy): the data dir's own folder, which the profile hides.
#[derive(Debug)]
pub struct SessionMark {
    tag: String,
    mark: PathBuf,
    canary: PathBuf,
    mark_c: CString,
    canary_c: CString,
    created: SystemTime,
    roots: Mutex<Vec<ProcId>>,
    told: AtomicBool,
}

impl SessionMark {
    /// Makes the mark of launch `tag` (a launcher's tag: `clauseo-` and then ASCII letters,
    /// digits or `-`) in `dir`, which must exist: the folder `<dir>/<tag>` (0700; an existing
    /// one is kept if it is a folder, never followed if it is a link) and the two files in it
    /// (0600, never through a link). The paths are canonical, as the kernel's check needs.
    pub fn create(dir: &Path, tag: &str) -> io::Result<SessionMark> {
        check_tag(tag).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        let dir = dunce::canonicalize(dir)?;
        if !std::fs::symlink_metadata(&dir)?.is_dir() {
            return Err(io::Error::other(format!(
                "{} is not a folder",
                dir.display()
            )));
        }
        let folder = dir.join(tag);
        make_folder(&folder)?;
        let mark = folder.join(MARK);
        let canary = folder.join(CANARY);
        make_file(&mark)?;
        make_file(&canary)?;
        let c = |path: &Path| CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other);
        Ok(SessionMark {
            tag: tag.to_owned(),
            mark_c: c(&mark)?,
            canary_c: c(&canary)?,
            mark,
            canary,
            created: now_wall(),
            roots: Mutex::new(Vec::new()),
            told: AtomicBool::new(false),
        })
    }

    /// The launch's tag.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The mark's path.
    pub fn path(&self) -> &Path {
        &self.mark
    }

    /// The canary's path.
    pub fn canary(&self) -> &Path {
        &self.canary
    }

    /// When the mark was made: no process of its launches started earlier.
    pub fn created(&self) -> SystemTime {
        self.created
    }

    /// The profile parameter naming the mark: `SESSION_MARK` and its path.
    pub fn param(&self) -> (String, PathBuf) {
        (SESSION_MARK.to_owned(), self.mark.clone())
    }

    /// Removes the two files and their folder. Only after an end answered
    /// [`Gone`](super::Gone): without the mark no scan can find the session's processes.
    pub fn remove(&self) -> io::Result<()> {
        remove(&self.canary, &self.mark)
    }

    /// The agents of this mark's launches that may still run.
    pub(crate) fn roots(&self) -> Vec<ProcId> {
        self.roots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Adds a launch's agent, dropping those that have exited.
    pub(crate) fn add_root(&self, root: ProcId) {
        let mut roots = self.roots.lock().unwrap_or_else(PoisonError::into_inner);
        roots.retain(|id| start_of(id.pid) == Some(id.start));
        roots.push(root);
    }

    /// The mark's path as the kernel takes it.
    pub(crate) fn mark_c(&self) -> &CStr {
        &self.mark_c
    }

    /// The canary's path as the kernel takes it.
    pub(crate) fn canary_c(&self) -> &CStr {
        &self.canary_c
    }

    /// Whether a scan failure is the first for this mark (the watchdog logs it once).
    pub(crate) fn tell_failure(&self) -> bool {
        !self.told.swap(true, Ordering::Relaxed)
    }

    /// Both files are still regular files (`lstat`), else [`SweepError::Mark`].
    pub(crate) fn check_files(&self) -> Result<(), SweepError> {
        for path in [&self.mark, &self.canary] {
            match std::fs::symlink_metadata(path) {
                Ok(meta) if meta.is_file() => {}
                Ok(_) => {
                    return Err(SweepError::Mark {
                        path: path.clone(),
                        source: io::Error::other("not a regular file"),
                    });
                }
                Err(source) => {
                    return Err(SweepError::Mark {
                        path: path.clone(),
                        source,
                    });
                }
            }
        }
        Ok(())
    }
}

/// Makes `folder` at 0700, or keeps it when it is a folder already (an earlier host with the
/// same pid made it); a link or a file there is refused.
#[expect(
    clippy::disallowed_methods,
    reason = "the session mark's folder, made by the host outside every jail"
)]
fn make_folder(folder: &Path) -> io::Result<()> {
    match DirBuilder::new().mode(0o700).create(folder) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if std::fs::symlink_metadata(folder)?.is_dir() {
                Ok(())
            } else {
                Err(io::Error::other(format!(
                    "{} is not a folder",
                    folder.display()
                )))
            }
        }
        Err(error) => Err(error),
    }
}

/// Makes `path` a regular file at 0600 if it is missing, never through a link
/// (`O_NOFOLLOW`), and checks what it opened is a regular file.
#[expect(
    clippy::disallowed_methods,
    reason = "the session mark and canary, made by the host outside every jail"
)]
fn make_file(path: &Path) -> io::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if file.metadata()?.is_file() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{} is not a regular file",
            path.display()
        )))
    }
}

/// Removes the canary, the mark and their folder; a file already gone is fine.
#[expect(
    clippy::disallowed_methods,
    reason = "the session mark's removal after its session is gone"
)]
fn remove(canary: &Path, mark: &Path) -> io::Result<()> {
    for path in [canary, mark] {
        match std::fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            Ok(()) | Err(_) => {}
        }
    }
    match mark.parent().map(std::fs::remove_dir) {
        Some(Err(error)) if error.kind() != io::ErrorKind::NotFound => Err(error),
        Some(Ok(()) | Err(_)) | None => Ok(()),
    }
}
