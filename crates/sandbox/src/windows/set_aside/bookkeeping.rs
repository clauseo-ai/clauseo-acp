//! A matter's set-aside bookkeeping deleted: by the per-user `--remove` as the owner once the
//! matter is forgotten (`setup/remove/ledger_work.rs`), never on the administrator's path (K1);
//! and by the uninstaller's `--delete-app-data` (the desktop crate), from a set-aside holding
//! nothing else. Every launch leaves `<data_dir>\sandbox\set-aside\<matter>\record.json` and
//! `set-aside.lock`; a pass that set something aside leaves `manifest.json`, which a restore of
//! every item empties ([`SET_ASIDE_BOOKKEEPING`]).
//!
//! It is all or nothing, in two passes, under the matter's `set-aside.lock`, taken before the
//! first pass and held to the deletes, as every writer takes it, so no writer changes the record
//! meanwhile. First every name in the matter's folder is checked: each must be one of those three
//! (matched ignoring case, as NTFS matches), a plain file (no reparse point, no folder) with one
//! link, and its contents must let it go ([`contents_allow`]): a record that lists no open
//! session, a manifest with nothing outstanding (no item set aside and not restored, no move
//! begun), and a manifest only beside such a record. Anything else (a pass folder, another name, a
//! hard-linked file, a record still open or a manifest still listing an item, either unreadable or
//! of another version) leaves the folder exactly as it is. Only then is each file deleted, and
//! last the folder, which goes only if empty. On NTFS a mark that fails partway (a read-only file)
//! takes the marks before it off again, so none goes. A record still open is kept because the next
//! launch checks a crashed session's changes only against it (`run.rs`): deleting it would let
//! what an agent planted become the next launch's trusted
//! starting point.
//!
//! On NTFS everything is opened by handle without following a link, each file is read and
//! checked on the handle that deletes it, and every handle stays open from the check to the
//! delete (`ntfs/forget.rs`). Off Windows, where only the tests run it, each folder on the way is
//! refused if a symbolic link, names match ignoring ASCII case, and each file is checked by its
//! own metadata.

use std::ffi::OsStr;
use std::path::Path;
#[cfg(any(windows, test))]
use std::time::Duration;

#[cfg(any(windows, test))]
use clauseo_protocol::MatterId;

use super::{SET_ASIDE_BOOKKEEPING, SET_ASIDE_DIR, SetAsideFailure};
use crate::windows::write;

/// The most a record or a manifest read here may hold: 64 MiB. A larger one is kept.
pub(super) const RECORD_CAP: usize = 64 << 20;

/// What became of a matter's set-aside folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetAsideBookkeeping {
    /// There was no folder for the matter.
    NoFolder,
    /// The bookkeeping and the folder are gone.
    Gone,
    /// The folder stays as it was: it holds something else, a record still open, a manifest
    /// still listing an item, either unreadable, or a bookkeeping file with another link.
    Left,
}

/// Whether `bytes`, a set-aside record, lists no open session: read as the record `files.rs`
/// writes (`Record`, its version the one written), with no session in it. Anything its typed
/// read refuses (a duplicate field, an unknown one, another version, unparsable) is open (kept).
pub(crate) fn set_aside_record_closed(bytes: &[u8]) -> bool {
    super::files::Record::closed(bytes)
}

/// The record's and the manifest's places in [`SET_ASIDE_BOOKKEEPING`]; the lock is the other.
pub(super) const RECORD_AT: usize = 0;
/// See [`RECORD_AT`].
pub(super) const MANIFEST_AT: usize = 2;

/// Whether the bookkeeping file at `at` in [`SET_ASIDE_BOOKKEEPING`] must be read before it
/// goes: the record and the manifest; the lock goes whatever it holds.
pub(super) fn read_first(at: usize) -> bool {
    at == RECORD_AT || at == MANIFEST_AT
}

/// Whether the contents `bytes` of the bookkeeping file at `at` let it go: a record that lists
/// no open session ([`set_aside_record_closed`]), a manifest with nothing outstanding (read as
/// the `Manifest` `files.rs` writes; anything its typed read refuses is outstanding).
pub(super) fn contents_allow(at: usize, bytes: &[u8]) -> bool {
    bytes.len() <= RECORD_CAP
        && match at {
            RECORD_AT => set_aside_record_closed(bytes),
            MANIFEST_AT => super::files::Manifest::settled_empty(bytes),
            _ => true,
        }
}

/// Whether the bookkeeping files found, by their places, may go together: a manifest only
/// beside a record (one whose contents allowed it).
pub(super) fn together(found: &[usize]) -> bool {
    !found.contains(&MANIFEST_AT) || found.contains(&RECORD_AT)
}

/// `--remove`'s call: deletes `matter`'s set-aside bookkeeping under `data_dir` (a data folder
/// already checked), reached from `<data_dir>\sandbox` with no link on the way, as the module
/// says, under the matter's lock (waiting as the writers do). A bookkeeping name that is a link
/// or a folder, or a folder on the way that is a link, is [`SetAsideFailure::Changed`] and nothing
/// more is deleted; a lock still held is `Io(WouldBlock)`, and nothing is either.
#[cfg(any(windows, test))]
pub(crate) fn forget_bookkeeping(
    data_dir: &Path,
    matter: &MatterId,
) -> Result<SetAsideBookkeeping, SetAsideFailure> {
    forget_bookkeeping_within(data_dir, matter, write::LOCK_WAIT)
}

/// [`forget_bookkeeping`], waiting up to `wait` for the matter's lock (the tests wait less).
#[cfg(any(windows, test))]
pub(super) fn forget_bookkeeping_within(
    data_dir: &Path,
    matter: &MatterId,
    wait: Duration,
) -> Result<SetAsideBookkeeping, SetAsideFailure> {
    let sandbox = data_dir.join(crate::windows::matters::SANDBOX_DIR);
    let name = matter.to_string();
    #[cfg(windows)]
    return super::ntfs::forget_from_sandbox(
        &sandbox,
        OsStr::new(&name),
        &SET_ASIDE_BOOKKEEPING,
        wait,
    );
    #[cfg(not(windows))]
    {
        let set_aside = sandbox.join(SET_ASIDE_DIR);
        posix::forget(&[&sandbox, &set_aside], &set_aside.join(name), wait)
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
    return super::ntfs::forget_from_set_aside(
        set_aside,
        matter,
        &SET_ASIDE_BOOKKEEPING,
        write::LOCK_WAIT,
    );
    #[cfg(not(windows))]
    posix::forget(&[set_aside], &set_aside.join(matter), write::LOCK_WAIT)
}

/// The tests' stand-in off Windows.
#[cfg(not(windows))]
mod posix {
    use std::io;
    use std::os::unix::fs::MetadataExt as _;
    use std::path::Path;
    use std::time::Duration;

    use super::{SET_ASIDE_BOOKKEEPING, SetAsideBookkeeping, contents_allow, read_first, together};
    use crate::windows::set_aside::SetAsideFailure;
    use crate::windows::set_aside::files::LOCK_FILE;
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

    /// Each of `way` checked as a real folder, then `folder`'s names checked as the module
    /// says, then each deleted, then `folder` when empty. The matter's lock is taken before its
    /// names are read, and held until the deletes are done, as on NTFS.
    pub(super) fn forget(
        way: &[&Path],
        folder: &Path,
        wait: Duration,
    ) -> Result<SetAsideBookkeeping, SetAsideFailure> {
        for step in way {
            if present(step, true)?.is_none() {
                return Ok(SetAsideBookkeeping::NoFolder);
            }
        }
        if present(folder, true)?.is_none() {
            return Ok(SetAsideBookkeeping::NoFolder);
        }
        let lock = folder.join(LOCK_FILE);
        if present(&lock, false)?.is_some_and(|meta| meta.nlink() > 1) {
            return Ok(SetAsideBookkeeping::Left);
        }
        let held = write::lock_exclusive_for(&lock, wait).map_err(|error| failure(&error))?;
        // First pass: every name checked; anything that isn't bookkeeping fit to go leaves the
        // folder as it is.
        let mut found = Vec::new();
        for entry in std::fs::read_dir(folder).map_err(|error| failure(&error))? {
            let path = entry.map_err(|error| failure(&error))?.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            let Some(at) = SET_ASIDE_BOOKKEEPING
                .iter()
                .position(|own| own.eq_ignore_ascii_case(name))
            else {
                return Ok(SetAsideBookkeeping::Left);
            };
            let Some(meta) = present(&path, false)? else {
                continue;
            };
            if meta.nlink() > 1 {
                return Ok(SetAsideBookkeeping::Left);
            }
            if read_first(at) {
                let bytes = std::fs::read(&path).map_err(|error| failure(&error))?;
                if !contents_allow(at, &bytes) {
                    return Ok(SetAsideBookkeeping::Left);
                }
            }
            found.push((at, path));
        }
        let places: Vec<usize> = found.iter().map(|(at, _)| *at).collect();
        if !together(&places) {
            return Ok(SetAsideBookkeeping::Left);
        }
        // Second pass: each deleted; then the lock goes, and the folder when empty.
        for (_, path) in found {
            write::remove_owned_file(&path).map_err(|error| failure(&error))?;
        }
        drop(held);
        match write::remove_empty_folder(folder) {
            Ok(()) => Ok(SetAsideBookkeeping::Gone),
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {
                Ok(SetAsideBookkeeping::Left)
            }
            Err(error) => Err(failure(&error)),
        }
    }
}
