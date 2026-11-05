//! The deletion itself: the two folders checked, then each walked without following a link
//! (pass 1) and removed (pass 2). See the contract in `delete_app_data.rs`.

use std::ffi::OsStr;
use std::fs::Metadata;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clauseo_settings::{IDENTIFIER, KnownFolder, PlatformDirs};

use super::{DELETED, FAILED, KEPT_SANDBOX, KEPT_SET_ASIDE, Mode, ops, say};

/// The sandbox's folder under the local data folder (`clauseo_sandbox::SANDBOX_DIR`, which the
/// desktop crate has only on Windows; a test holds the two equal there).
pub(super) const SANDBOX_DIR: &str = "sandbox";
/// Where tier B moves the lawyer's protected names aside, under [`SANDBOX_DIR`].
pub(super) const SET_ASIDE_DIR: &str = "set-aside";

/// Deletes both data folders as `mode` says, the known folders from `dirs`; answers the exit
/// code.
pub(crate) fn delete(dirs: &dyn PlatformDirs, mode: Mode, stderr: &mut dyn Write) -> i32 {
    let folders = [KnownFolder::LocalAppData, KnownFolder::RoamingAppData].map(|folder| {
        dirs.known_folder(folder)
            .map(|known| (known.join(IDENTIFIER), known))
            .ok_or(folder)
    });
    let mut present = Vec::new();
    for (index, folder) in folders.into_iter().enumerate() {
        let (root, known) = match folder {
            Ok(found) => found,
            Err(folder) => {
                say(
                    stderr,
                    &format!("Windows gave no {folder:?} folder. Nothing deleted."),
                );
                return FAILED;
            }
        };
        match check(&known, &root) {
            Ok(true) => present.push((index == 0, root)),
            Ok(false) => {}
            Err(why) => {
                say(
                    stderr,
                    &format!("{}: {why}. Nothing deleted.", root.display()),
                );
                return FAILED;
            }
        }
    }
    let mut report = Report::new(stderr);
    let mut kept = None;
    for (local, root) in present {
        let keep = if local {
            kept_folder(&root, mode)
        } else {
            None
        };
        unlink_links(&root, keep.as_deref(), &mut report);
        remove_rest(&root, keep.as_deref(), &mut report);
        if local {
            kept = keep.filter(|keep| std::fs::symlink_metadata(keep).is_ok());
        }
    }
    if report.failures > 0 {
        return FAILED;
    }
    match kept {
        None => DELETED,
        Some(keep) if keep.ends_with(SET_ASIDE_DIR) => KEPT_SET_ASIDE,
        Some(_) => KEPT_SANDBOX,
    }
}

/// Whether `root` is there to delete: `Ok(false)` when it does not exist, the reason it is
/// refused otherwise. It must be `known`'s child named [`IDENTIFIER`], a real folder (no reparse
/// point), whose real path is directly in `known`'s real path.
pub(super) fn check(known: &Path, root: &Path) -> Result<bool, String> {
    if !known.is_absolute() {
        return Err(format!("{} is not an absolute path", known.display()));
    }
    if root.parent() != Some(known) || root.file_name() != Some(OsStr::new(IDENTIFIER)) {
        return Err(format!("not {IDENTIFIER} directly in {}", known.display()));
    }
    let meta = match std::fs::symlink_metadata(root) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("could not be read ({error})")),
    };
    if is_link(&meta) {
        return Err("is a link (a junction or a symbolic link)".to_owned());
    }
    if !meta.is_dir() {
        return Err("is not a folder".to_owned());
    }
    let real = |path: &Path| {
        dunce::canonicalize(path).map_err(|error| format!("{}: {error}", path.display()))
    };
    let (real_known, real_root) = (real(known)?, real(root)?);
    if real_root.parent() != Some(real_known.as_path()) {
        return Err(format!(
            "its real path {} is not directly in {}",
            real_root.display(),
            real_known.display()
        ));
    }
    Ok(true)
}

/// The folder under the local data folder `root` that stays: `sandbox` with `--keep-sandbox`
/// when it exists, else `sandbox\set-aside` when it holds anything (or is anything but a real,
/// empty folder: what it is is not looked into).
fn kept_folder(root: &Path, mode: Mode) -> Option<PathBuf> {
    let sandbox = root.join(SANDBOX_DIR);
    let sandbox_meta = std::fs::symlink_metadata(&sandbox).ok()?;
    if mode.keep_sandbox {
        return Some(sandbox);
    }
    // A `sandbox` that is a link goes as a link, and nothing through it is looked at.
    if is_link(&sandbox_meta) || !sandbox_meta.is_dir() {
        return None;
    }
    let set_aside = sandbox.join(SET_ASIDE_DIR);
    let meta = std::fs::symlink_metadata(&set_aside).ok()?;
    let empty = !is_link(&meta)
        && meta.is_dir()
        && std::fs::read_dir(&set_aside).is_ok_and(|mut entries| entries.next().is_none());
    (!empty).then_some(set_aside)
}

/// Pass 1: every reparse point under `root` (but `keep`) removed as a link and never entered;
/// read-only entries made writable. Iterative, so no depth overflows the stack.
fn unlink_links(root: &Path, keep: Option<&Path>, report: &mut Report<'_>) {
    ops::clear_read_only(root, report);
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        for (path, meta) in entries(&folder, report) {
            if Some(path.as_path()) == keep {
                continue;
            }
            if is_link(&meta) {
                report.check(&path, ops::remove_link(&path, &meta));
            } else {
                ops::clear_read_only_meta(&path, &meta, report);
                if meta.is_dir() {
                    folders.push(path);
                }
            }
        }
    }
}

/// Pass 2: `root` removed with std's `remove_dir_all`. When `keep` is under it, or something
/// will not go, its folders are taken a level at a time: each child that is no folder removed
/// alone, each folder by `remove_dir_all` again, a link as a link; then the emptied folders,
/// deepest first, but never `keep` or a folder above it.
fn remove_rest(root: &Path, keep: Option<&Path>, report: &mut Report<'_>) {
    if keep.is_none() && ops::remove_dir_all(root).is_ok() {
        return;
    }
    let above_keep = |path: &Path| keep.is_some_and(|keep| keep.starts_with(path));
    let mut opened = Vec::new();
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        for (path, meta) in entries(&folder, report) {
            if Some(path.as_path()) == keep {
                continue;
            }
            if is_link(&meta) {
                report.check(&path, ops::remove_link(&path, &meta));
            } else if !meta.is_dir() {
                report.check(&path, ops::remove_file(&path));
            } else if above_keep(&path) || ops::remove_dir_all(&path).is_err() {
                folders.push(path);
            }
        }
        opened.push(folder);
    }
    for folder in opened.iter().rev().filter(|folder| !above_keep(folder)) {
        if let Err(error) = ops::remove_dir(folder) {
            // A folder still holding something that failed is already reported.
            let empty = std::fs::read_dir(folder).is_ok_and(|mut left| left.next().is_none());
            if empty {
                report.fail(folder, &error);
            }
        }
    }
}

/// `folder`'s entries with their own metadata (a link's, never its target's). A failure to list
/// or to read an entry is reported, and the entry skipped.
fn entries(folder: &Path, report: &mut Report<'_>) -> Vec<(PathBuf, Metadata)> {
    let listed = match std::fs::read_dir(folder) {
        Ok(listed) => listed,
        Err(error) => {
            report.fail(folder, &error);
            return Vec::new();
        }
    };
    let mut found = Vec::new();
    for entry in listed {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(error) => {
                report.fail(folder, &error);
                continue;
            }
        };
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => found.push((path, meta)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => report.fail(&path, &error),
        }
    }
    found
}

/// `FILE_ATTRIBUTE_REPARSE_POINT`.
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// Whether `meta` (from `symlink_metadata`) is a reparse point of any kind: a junction, a
/// symbolic link to a folder or a file, a mount point.
#[cfg(windows)]
pub(super) fn is_link(meta: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Whether `meta` (from `symlink_metadata`) is a symbolic link: the tests' stand-in off Windows.
#[cfg(not(windows))]
pub(super) fn is_link(meta: &Metadata) -> bool {
    meta.file_type().is_symlink()
}

/// What failed, counted and written one line each.
pub(super) struct Report<'a> {
    stderr: &'a mut dyn Write,
    pub(super) failures: usize,
}

impl<'a> Report<'a> {
    fn new(stderr: &'a mut dyn Write) -> Self {
        Self {
            stderr,
            failures: 0,
        }
    }

    /// Counts and writes a failure at `path`.
    pub(super) fn fail(&mut self, path: &Path, error: &io::Error) {
        self.failures += 1;
        say(
            self.stderr,
            &format!("{}: could not delete it ({error})", path.display()),
        );
    }

    /// Reports `result`'s failure at `path`, if any; a path already gone is no failure.
    fn check(&mut self, path: &Path, result: io::Result<()>) {
        match result {
            Err(error) if error.kind() != io::ErrorKind::NotFound => self.fail(path, &error),
            _ => {}
        }
    }

    /// Writes a line that is not a failure.
    #[cfg(windows)]
    pub(super) fn note(&mut self, line: &str) {
        say(self.stderr, line);
    }
}
