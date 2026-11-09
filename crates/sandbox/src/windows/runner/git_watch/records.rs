//! The records `ReadDirectoryChangesW` and `ReadDirectoryChangesExW` fill the watch's buffer
//! with: `FILE_NOTIFY_INFORMATION`, or with `ReadDirectoryNotifyExtendedInformation`
//! `FILE_NOTIFY_EXTENDED_INFORMATION`, which also gives each item's attributes, so a folder
//! that became a junction (its `FILE_ACTION_MODIFIED` with `FILE_ATTRIBUTE_REPARSE_POINT`) is
//! told from the many writes the same filter reports, and each item's creation time and file ID,
//! so a folder moved in from another folder is told from one just made (both come as
//! `FILE_ACTION_ADDED`): by its age, or by its file ID leaving another place (`pairs.rs`).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// `FILE_ACTION_ADDED`.
const ADDED: u32 = 1;
/// `FILE_ACTION_REMOVED`.
const REMOVED: u32 = 2;
/// `FILE_ACTION_RENAMED_OLD_NAME`.
const RENAMED_OLD_NAME: u32 = 4;
/// `FILE_ACTION_MODIFIED`.
const MODIFIED: u32 = 3;
/// `FILE_ACTION_RENAMED_NEW_NAME`.
const RENAMED_NEW_NAME: u32 = 5;
/// `FILE_ATTRIBUTE_REPARSE_POINT`.
const REPARSE_POINT: u32 = 0x400;
/// `FILE_ATTRIBUTE_DIRECTORY`.
const DIRECTORY: u32 = 0x10;
/// How much older than the watch's read a folder reported `FILE_ACTION_ADDED` must have been
/// created to count as moved in, not made: 5 s, in a FILETIME's 100 ns ticks. A folder just
/// made is created moments before its record is read; one moved in keeps its creation time.
pub(crate) const MOVED_IN_AGE: u64 = 5 * 10_000_000;
/// The FILETIME of the Unix epoch: 100 ns ticks since 1601-01-01.
const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
/// Where an extended record's `CreationTime` is.
const CREATION_AT: usize = 8;
/// Where an extended record's `FileId` is.
const FILE_ID_AT: usize = 64;

/// Where a record's fields are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// `FILE_NOTIFY_INFORMATION`: no attributes, so every `FILE_ACTION_MODIFIED` is checked.
    Plain,
    /// `FILE_NOTIFY_EXTENDED_INFORMATION`.
    Extended,
}

impl Layout {
    /// The offsets of the name's length and of the name; with the attributes', when it has them.
    fn offsets(self) -> (usize, usize, Option<usize>) {
        match self {
            Layout::Plain => (8, 12, None),
            Layout::Extended => (80, 84, Some(56)),
        }
    }
}

/// What a buffer reported, each path relative to the watched folder.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Changes {
    /// Names made or renamed in.
    pub(crate) added: Vec<PathBuf>,
    /// Items changed that are reparse points now (with [`Layout::Plain`], every item changed).
    pub(crate) relinked: Vec<PathBuf>,
    /// Of `added`, each folder renamed or moved in: one gone by the check counts as gone
    /// (`set_aside::live`). Renamed in within its folder is `FILE_ACTION_RENAMED_NEW_NAME`.
    /// Moved in from another folder comes as `FILE_ACTION_ADDED`, as `mkdir` does, but keeps
    /// its creation time (confirmed on a Windows Server 2025 VM, NTFS; `mkdir`'s is within about
    /// 4 ms of the record), so it is told by a creation time more than [`MOVED_IN_AGE`] before
    /// `now`, when the buffer was received. A folder just made isn't listed: whatever is put in
    /// it is reported on its own, so `mkdir tmp` then `rm -r tmp` hides nothing. One whose
    /// creation time is in the future or unreadable is listed. Empty with [`Layout::Plain`],
    /// which has no attributes or times.
    pub(crate) moved_folders: Vec<PathBuf>,
    /// Of `added`, each folder made just now by its creation time, with its file ID: one gone
    /// by the check counts as gone only when that file ID left another place (`pairs.rs`), so
    /// a forged creation time doesn't hide a folder moved in. Not listed without a file ID.
    pub(crate) fresh_folders: Vec<(PathBuf, u64)>,
    /// Each item removed or renamed away (`FILE_ACTION_REMOVED`, `FILE_ACTION_RENAMED_OLD_NAME`),
    /// with its file ID, for the pairing: none without a file ID, nor with [`Layout::Plain`].
    pub(crate) left: Vec<(PathBuf, u64)>,
}

impl Changes {
    /// The paths of [`Changes::fresh_folders`].
    pub(crate) fn fresh_paths(&self) -> Vec<PathBuf> {
        self.fresh_folders
            .iter()
            .map(|(path, _)| path.clone())
            .collect()
    }
}

/// `at` as a FILETIME: 100 ns ticks since 1601-01-01; 0 when it is before 1970 or past what a
/// FILETIME holds, so every folder added counts as moved in.
pub(crate) fn filetime(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| u64::try_from(since.as_nanos() / 100).ok())
        .and_then(|ticks| ticks.checked_add(UNIX_EPOCH_TICKS))
        .unwrap_or(0)
}

/// Whether a folder reported added at `created` (a FILETIME) counts as moved in: unless it was
/// made at most [`MOVED_IN_AGE`] before `now`, it does. A creation time after `now` (clock skew,
/// or forged) or unreadable (0 or less) counts too: the count fails toward ending the session.
fn moved_in(created: i64, now: u64) -> bool {
    let Some(created) = u64::try_from(created).ok().filter(|created| *created > 0) else {
        return true;
    };
    now.checked_sub(created)
        .is_none_or(|age| age > MOVED_IN_AGE)
}

/// What the records in `buffer` (chained by `NextEntryOffset`) report, received at `now` (a
/// FILETIME, [`filetime`], read once per buffer as it arrives). `None` when the buffer doesn't
/// read as records: the watch then rescans.
pub(crate) fn changes(buffer: &[u8], layout: Layout, now: u64) -> Option<Changes> {
    let (length_at, name_at, attributes_at) = layout.offsets();
    let mut changes = Changes::default();
    let mut at = 0_usize;
    loop {
        let record = buffer.get(at..)?;
        let next = word(record, 0)?;
        let action = word(record, 4)?;
        let length = usize::try_from(word(record, length_at)?).ok()?;
        let name = record.get(name_at..name_at.checked_add(length)?)?;
        let attributes = match attributes_at {
            Some(offset) => Some(word(record, offset)?),
            None => None,
        };
        let reparse = attributes.is_none_or(|bits| bits & REPARSE_POINT != 0);
        let folder = attributes.is_some_and(|bits| bits & DIRECTORY != 0);
        match action {
            ADDED | RENAMED_NEW_NAME => {
                let path = path_of(name)?;
                let counted = action == RENAMED_NEW_NAME
                    || (attributes.is_some() && moved_in(signed(record, CREATION_AT)?, now));
                if folder && counted {
                    changes.moved_folders.push(path.clone());
                } else if folder && let Some(id) = file_id(record) {
                    changes.fresh_folders.push((path.clone(), id));
                }
                changes.added.push(path);
            }
            REMOVED | RENAMED_OLD_NAME if attributes.is_some() => {
                if let Some(id) = file_id(record) {
                    changes.left.push((path_of(name)?, id));
                }
            }
            MODIFIED if reparse => changes.relinked.push(path_of(name)?),
            _ => {}
        }
        if next == 0 {
            return Some(changes);
        }
        at = at.checked_add(usize::try_from(next).ok()?)?;
    }
}

/// The little-endian `u32` at `at` in `bytes`.
fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let four: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(four))
}

/// An extended record's file ID; `None` when it is 0 (none given) or the record is too short.
fn file_id(record: &[u8]) -> Option<u64> {
    let id = signed(record, FILE_ID_AT)?;
    (id != 0).then(|| u64::from_le_bytes(id.to_le_bytes()))
}

/// The little-endian `i64` at `at` in `bytes`.
fn signed(bytes: &[u8], at: usize) -> Option<i64> {
    let eight: [u8; 8] = bytes.get(at..at.checked_add(8)?)?.try_into().ok()?;
    Some(i64::from_le_bytes(eight))
}

/// A name in a record: UTF-16LE, kept as it is on Windows, text or not.
fn path_of(name: &[u8]) -> Option<PathBuf> {
    let (pairs, odd) = name.as_chunks::<2>();
    if !odd.is_empty() {
        return None;
    }
    let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt as _;
        Some(PathBuf::from(std::ffi::OsString::from_wide(&units)))
    }
    #[cfg(not(windows))]
    {
        String::from_utf16(&units).ok().map(PathBuf::from)
    }
}
