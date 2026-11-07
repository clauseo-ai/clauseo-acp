//! A matter's set-aside bookkeeping deleted: by the per-user `--remove` as the owner once the
//! matter is forgotten (`setup/remove/ledger_work.rs`), never on the administrator's path (K1);
//! and by the uninstaller's `--delete-app-data` (the desktop crate), from a set-aside holding
//! nothing else. Every launch leaves `<data_dir>\sandbox\set-aside\<matter>\record.json` and
//! `set-aside.lock` ([`SET_ASIDE_BOOKKEEPING`]).
//!
//! Only those two names go, each only as a plain file (no reparse point, no folder, one link
//! only: a hard-linked one leaves the folder as it is), then the matter's folder, only when
//! nothing else is in it. A record that still lists an open session, or can't be read or
//! parsed, is kept with the rest: the next launch checks a crashed session's changes only
//! against it (`run.rs`), so deleting it would let what an agent planted become the next
//! launch's trusted starting point. The lawyer's files a pass moved aside, and the manifest
//! listing them, stay.
//!
//! On NTFS everything is opened by handle without following a link, and the record is read on
//! the handle that deletes it (`ntfs/forget.rs`). Off Windows, where only the tests run it, each
//! folder on the way is refused if a symbolic link, names match ignoring ASCII case as NTFS
//! does, and each file is checked by its own metadata.

use std::ffi::OsStr;
use std::path::Path;

#[cfg(any(windows, test))]
use clauseo_protocol::MatterId;

use super::{SET_ASIDE_BOOKKEEPING, SET_ASIDE_DIR, SetAsideFailure};

/// The most a record read here may hold: 64 MiB. A larger one is kept.
pub(super) const RECORD_CAP: usize = 64 << 20;

/// What became of a matter's set-aside folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetAsideBookkeeping {
    /// There was no folder for the matter.
    NoFolder,
    /// The bookkeeping and the folder are gone.
    Gone,
    /// The folder stays: it holds something else, a record still open or unreadable, or a
    /// bookkeeping file with another link. Whatever bookkeeping could go went.
    Left,
}

/// Whether `bytes`, a set-aside record, lists no open session: a JSON object of version 1 whose
/// `sessions` is an empty list. Anything else, unparsable included, is open (kept).
pub fn set_aside_record_closed(bytes: &[u8]) -> bool {
    let Ok(serde_json::Value::Object(record)) = serde_json::from_slice(bytes) else {
        return false;
    };
    record.get("version").and_then(serde_json::Value::as_u64) == Some(1)
        && record
            .get("sessions")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty)
}

/// `--remove`'s call: deletes `matter`'s set-aside bookkeeping under `data_dir` (a data folder
/// already checked), reached from `<data_dir>\sandbox` with no link on the way, as the module
/// says. A bookkeeping name that is a link or a folder, or a folder on the way that is a link,
/// is [`SetAsideFailure::Changed`] and nothing more is deleted.
#[cfg(any(windows, test))]
pub(crate) fn forget_bookkeeping(
    data_dir: &Path,
    matter: &MatterId,
) -> Result<SetAsideBookkeeping, SetAsideFailure> {
    let sandbox = data_dir.join(crate::windows::matters::SANDBOX_DIR);
    let name = matter.to_string();
    #[cfg(windows)]
    return super::ntfs::forget_from_sandbox(&sandbox, OsStr::new(&name), &SET_ASIDE_BOOKKEEPING);
    #[cfg(not(windows))]
    {
        let set_aside = sandbox.join(SET_ASIDE_DIR);
        posix::forget(&[&sandbox, &set_aside], &set_aside.join(name))
    }
}

/// The uninstaller's call: deletes the bookkeeping of the matter whose folder in `set_aside`
/// (the data folder's `sandbox\set-aside`, its `sandbox` held by the caller as no link) is named
/// `matter`, as [`forget_bookkeeping`] does; the set-aside folder itself is opened by its path
/// as itself and refused if a link.
pub fn forget_set_aside_bookkeeping(
    set_aside: &Path,
    matter: &OsStr,
) -> Result<SetAsideBookkeeping, SetAsideFailure> {
    debug_assert!(set_aside.ends_with(SET_ASIDE_DIR));
    #[cfg(windows)]
    return super::ntfs::forget_from_set_aside(set_aside, matter, &SET_ASIDE_BOOKKEEPING);
    #[cfg(not(windows))]
    posix::forget(&[set_aside], &set_aside.join(matter))
}

/// The tests' stand-in off Windows.
#[cfg(not(windows))]
mod posix {
    use std::io;
    use std::os::unix::fs::MetadataExt as _;
    use std::path::Path;

    use super::{RECORD_CAP, SET_ASIDE_BOOKKEEPING, SetAsideBookkeeping, set_aside_record_closed};
    use crate::windows::set_aside::SetAsideFailure;
    use crate::windows::write;

    fn failure(error: &io::Error) -> SetAsideFailure {
        SetAsideFailure::Io(error.kind())
    }

    /// Whether `path` is there as a real folder (`folder`) or a plain file; a symbolic link or
    /// the other kind is [`SetAsideFailure::Changed`].
    fn present(path: &Path, folder: bool) -> Result<Option<std::fs::Metadata>, SetAsideFailure> {
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() || meta.is_dir() != folder => {
                Err(SetAsideFailure::Changed)
            }
            Ok(meta) => Ok(Some(meta)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(failure(&error)),
        }
    }

    /// Each of `way` checked as a real folder, then the bookkeeping in `folder` deleted, then
    /// `folder` when empty.
    pub(super) fn forget(
        way: &[&Path],
        folder: &Path,
    ) -> Result<SetAsideBookkeeping, SetAsideFailure> {
        for step in way {
            if present(step, true)?.is_none() {
                return Ok(SetAsideBookkeeping::NoFolder);
            }
        }
        if present(folder, true)?.is_none() {
            return Ok(SetAsideBookkeeping::NoFolder);
        }
        // Each bookkeeping name as listed, matched ignoring case, as NTFS opens it.
        let mut found = Vec::new();
        for entry in std::fs::read_dir(folder).map_err(|error| failure(&error))? {
            let path = entry.map_err(|error| failure(&error))?.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if let Some(at) = SET_ASIDE_BOOKKEEPING
                .iter()
                .position(|own| own.eq_ignore_ascii_case(name))
            {
                found.push((at, path));
            }
        }
        // The record first: one still open keeps everything.
        found.sort_by_key(|(at, _)| *at);
        for (at, path) in found {
            let Some(meta) = present(&path, false)? else {
                continue;
            };
            if meta.nlink() > 1 {
                return Ok(SetAsideBookkeeping::Left);
            }
            if at == 0 {
                let bytes = std::fs::read(&path).map_err(|error| failure(&error))?;
                if bytes.len() > RECORD_CAP || !set_aside_record_closed(&bytes) {
                    return Ok(SetAsideBookkeeping::Left);
                }
            }
            write::remove_owned_file(&path).map_err(|error| failure(&error))?;
        }
        match write::remove_empty_folder(folder) {
            Ok(()) => Ok(SetAsideBookkeeping::Gone),
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {
                Ok(SetAsideBookkeeping::Left)
            }
            Err(error) => Err(failure(&error)),
        }
    }
}
