//! The deletion itself: the two folders checked, then each emptied a level at a time, every
//! entry removed by std (a link as a link, a folder with `remove_dir_all`), descending by hand
//! only toward the folder that stays. See the contract in `delete_app_data.rs`.

use std::ffi::OsStr;
use std::fs::Metadata;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clauseo_settings::{IDENTIFIER, KnownFolder, PlatformDirs};

use super::{DELETED, FAILED, KEPT_SANDBOX, KEPT_SET_ASIDE, Mode, ops, say};

/// The sandbox's folder under the local data folder: S15's records (its ledger, the matter
/// SIDs, and the clean-close records of S15's third PR, part d), which its `--remove` needs.
/// Kept whole unless `--sandbox-removed`, so a record added there later is kept too.
pub(super) const SANDBOX_DIR: &str = clauseo_sandbox::windows::matters::SANDBOX_DIR;
/// The set-aside's own name, under [`SANDBOX_DIR`].
// TODO(#465): use clauseo_sandbox::windows::SET_ASIDE_DIR
pub(super) const SET_ASIDE_DIR: &str = "set-aside";
/// Where tier B moves the lawyer's protected names aside, under the local data folder, one name
/// per level: `<data>\sandbox\set-aside\<matter>\<timestamp>\…` (S15, #464). The one place
/// its location is written; the hook's message path is held to it by a test.
pub(super) const SET_ASIDE_PATH: [&str; 2] = [SANDBOX_DIR, SET_ASIDE_DIR];

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
        // The way down to the folder that stays, in the local folder only: `sandbox` whole
        // unless S15's removal ran, else a `set-aside` that holds anything.
        let toward: &[&str] = match (local, mode.sandbox_removed) {
            (false, _) => &[],
            (true, false) => &[SANDBOX_DIR],
            (true, true) => &SET_ASIDE_PATH,
        };
        // `sandbox` stays whole; the set-aside stays unless it is empty.
        let whole = !mode.sandbox_removed;
        if let Ok(stays) = empty_folder(&root, toward, whole, &mut report) {
            if stays.is_none() {
                remove_emptied(&root, &mut report);
            }
            if local {
                kept = stays;
            }
        }
    }
    // With --sandbox-removed only the set-aside can stay; without it, `sandbox`.
    match kept {
        Some(_) if mode.sandbox_removed => KEPT_SET_ASIDE,
        Some(_) => KEPT_SANDBOX,
        None if report.failures > 0 => FAILED,
        None => DELETED,
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
    // Both without a `\\?\` prefix: `dunce` keeps it only on a path it cannot shorten (past
    // 260 characters), so the data folder could have it while its known folder does not.
    let (real_known, real_root) = (plain(&real(known)?), plain(&real(root)?));
    if real_root.parent() != Some(real_known.as_path()) {
        return Err(format!(
            "its real path {} is not directly in {}",
            real_root.display(),
            real_known.display()
        ));
    }
    Ok(true)
}

/// `path` without a verbatim prefix: `\\?\C:\…` as `C:\…`, `\\?\UNC\server\share\…` as
/// `\\server\share\…`; any other path as it is (every path off Windows).
pub(super) fn plain(path: &Path) -> PathBuf {
    use std::path::{Component, Prefix};
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path.to_owned();
    };
    let mut out = match prefix.kind() {
        Prefix::VerbatimDisk(drive) => std::ffi::OsString::from(format!("{}:", char::from(drive))),
        Prefix::VerbatimUNC(server, share) => {
            let mut unc = std::ffi::OsString::from(r"\\");
            unc.push(server);
            unc.push(r"\");
            unc.push(share);
            unc
        }
        _ => return path.to_owned(),
    };
    out.push(components.as_path().as_os_str());
    PathBuf::from(out)
}

/// Removes everything in `folder` but the folder that stays, which `toward` names below it
/// (`[]` for none). `folder` is first held open by handle, which also says it is no reparse
/// point (`ops::pin_folder`); while it is held it cannot be renamed, deleted or replaced. Each
/// entry not on the way down is removed by std, never entered here: a reparse point as a link,
/// a folder with `remove_dir_all` (which opens nothing through a link), a file with
/// `remove_file`. Only the folder on the way down is entered, the same way. What is at the end
/// of the way down stays as [`stays_whole`] says with `whole`. Answers the folder that stays,
/// if one does; `Err` when `folder` was not entered (reported).
fn empty_folder(
    folder: &Path,
    toward: &[&str],
    whole: bool,
    report: &mut Report<'_>,
) -> Result<Option<PathBuf>, ()> {
    // Held until the folder's entries are done, then closed.
    let _pinned = match ops::pin_folder(folder) {
        Ok(pinned) => pinned,
        Err(error) => {
            report.fail(folder, &error);
            return Err(());
        }
    };
    let mut stays = None;
    for (path, meta) in entries(folder, report) {
        // Windows file names ignore case: `Sandbox` is `sandbox`. What stays is the entry as
        // listed.
        let next = toward.split_first().filter(|(name, _)| named(&path, name));
        if let Some((_, rest)) = next {
            if rest.is_empty() && stays_whole(&path, &meta, whole) {
                stays = Some(path);
                continue;
            }
            if !rest.is_empty() && !is_link(&meta) && meta.is_dir() {
                if let Ok(below) = empty_folder(&path, rest, whole, report) {
                    if below.is_none() {
                        remove_emptied(&path, report);
                    }
                    stays = below.or(stays);
                }
                continue;
            }
            // A link, a file, or an empty `set-aside`: removed like any other entry.
        }
        remove_entry(&path, &meta, report);
    }
    Ok(stays)
}

/// Whether `path`'s last name is `name`, ignoring case as Windows does.
fn named(path: &Path, name: &str) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|listed| listed.eq_ignore_ascii_case(name))
}

/// Whether the entry at the end of the way down stays as it is: always when `whole` (`sandbox`:
/// whatever it is, it is not looked into), else (the set-aside) unless it is a real, empty
/// folder.
fn stays_whole(path: &Path, meta: &Metadata, whole: bool) -> bool {
    if whole {
        return true;
    }
    let empty = !is_link(meta)
        && meta.is_dir()
        && std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none());
    !empty
}

/// Removes one entry by std: a reparse point as a link, a folder with `remove_dir_all`, a file
/// with `remove_file`. Whatever fails is reported, and nothing below it is tried by path.
fn remove_entry(path: &Path, meta: &Metadata, report: &mut Report<'_>) {
    let removed = if is_link(meta) {
        ops::remove_link(path, meta)
    } else if meta.is_dir() {
        ops::remove_dir_all(path)
    } else {
        ops::remove_file(path)
    };
    report.check(path, removed);
}

/// Removes the folder `folder`, emptied by [`empty_folder`], with std's `remove_dir_all` (by
/// handle, read-only included). When something is left in it, that was already reported.
fn remove_emptied(folder: &Path, report: &mut Report<'_>) {
    if let Err(error) = ops::remove_dir_all(folder) {
        let empty = std::fs::read_dir(folder).is_ok_and(|mut left| left.next().is_none());
        if empty {
            report.fail(folder, &error);
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
pub(super) const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

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
}
