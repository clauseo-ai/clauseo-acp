//! The records `ReadDirectoryChangesW` and `ReadDirectoryChangesExW` fill the watch's buffer
//! with: `FILE_NOTIFY_INFORMATION`, or with `ReadDirectoryNotifyExtendedInformation`
//! `FILE_NOTIFY_EXTENDED_INFORMATION`, which also gives each item's attributes, so a folder
//! that became a junction (its `FILE_ACTION_MODIFIED` with `FILE_ATTRIBUTE_REPARSE_POINT`) is
//! told from the many writes the same filter reports.

use std::path::PathBuf;

/// `FILE_ACTION_ADDED`.
const ADDED: u32 = 1;
/// `FILE_ACTION_MODIFIED`.
const MODIFIED: u32 = 3;
/// `FILE_ACTION_RENAMED_NEW_NAME`.
const RENAMED_NEW_NAME: u32 = 5;
/// `FILE_ATTRIBUTE_REPARSE_POINT`.
const REPARSE_POINT: u32 = 0x400;
/// `FILE_ATTRIBUTE_DIRECTORY`.
const DIRECTORY: u32 = 0x10;

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
    /// Of `added`, each that was a folder when reported: one gone by the check counts as gone
    /// (`set_aside::live`). Empty with [`Layout::Plain`], which has no attributes to tell by.
    pub(crate) folders: Vec<PathBuf>,
}

/// What the records in `buffer` (chained by `NextEntryOffset`) report. `None` when the buffer
/// doesn't read as records: the watch then rescans.
pub(crate) fn changes(buffer: &[u8], layout: Layout) -> Option<Changes> {
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
                if folder {
                    changes.folders.push(path.clone());
                }
                changes.added.push(path);
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
