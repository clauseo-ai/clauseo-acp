//! The change journal on NTFS: [`NtfsJournal`], the [`Journal`] a launch uses on Windows, at the
//! lawyer's Medium token. Every handle here opens a path as itself, never through a link, and
//! asks only to read attributes (a folder's listing too, for [`Journal::tree`]).
//!
//! The records are read with `FSCTL_READ_UNPRIVILEGED_USN_JOURNAL` (0x903AB) on a handle on the
//! own root, which needs no administrator rights and gives each record without its name.
//! Microsoft doesn't document it on Learn. The Windows SDK 10.0.14393.0 `winioctl.h` defines it
//! under `#if (_WIN32_WINNT >= _WIN32_WINNT_WIN10_RS1)` (Windows 10 1607, build 14393), and SDK
//! 10.0.10240.0 lacks it:
//! <https://raw.githubusercontent.com/tpn/winsdk-10/master/Include/10.0.14393.0/um/winioctl.h>
//! (a mirror of the SDK headers, not a Microsoft page). On an older build the call fails, and
//! the caller does the full walk.
//!
//! Every doubtful answer is an error, which also means the full walk: a short read, a record
//! that doesn't fit, a cursor that doesn't advance, a folder that can't be listed.
#![expect(
    unsafe_code,
    reason = "FFI to the change journal's FSCTLs on a handle on an own root"
)]

use std::ffi::OsString;
use std::io;
use std::mem::offset_of;
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_UNPRIVILEGED_USN_JOURNAL, READ_USN_JOURNAL_DATA_V1,
    USN_JOURNAL_DATA_V0, USN_RECORD_V2,
};

use super::{Change, Flow, Found, Journal, JournalError, JournalState, Listed, RootFacts, Usn};
use crate::windows::acl::object::{Handle, Object, os_error, reparse_refused};
use crate::windows::writer::{ApplyFailure, FileId};

mod volume;

/// `ERROR_INVALID_FUNCTION`.
const INVALID_FUNCTION: i32 = 1;
/// `ERROR_NOT_SUPPORTED`.
const NOT_SUPPORTED: i32 = 50;
/// `ERROR_INVALID_PARAMETER`.
const INVALID_PARAMETER: i32 = 87;
/// `ERROR_JOURNAL_DELETE_IN_PROGRESS`.
const JOURNAL_DELETE_IN_PROGRESS: i32 = 1178;
/// `ERROR_JOURNAL_NOT_ACTIVE`.
const JOURNAL_NOT_ACTIVE: i32 = 1179;
/// `ERROR_JOURNAL_ENTRY_DELETED`.
const JOURNAL_ENTRY_DELETED: i32 = 1181;

/// A read's output buffer, in u64 words: 64 KB.
const READ_WORDS: usize = 8192;

/// The change journal and the trees' file IDs on NTFS.
#[derive(Debug, Clone, Copy, Default)]
pub struct NtfsJournal;

impl Journal for NtfsJournal {
    fn root(&self, path: &Path) -> io::Result<RootFacts> {
        volume::facts(path)
    }

    fn query(&self, root: &Path) -> Result<JournalState, JournalError> {
        let handle = open_root(root)?;
        if !volume::is_ntfs(&handle).map_err(|e| JournalError::Failed(e.kind()))? {
            return Err(JournalError::NotNtfs);
        }
        // Room for `USN_JOURNAL_DATA_V2`; only the V0 prefix is read.
        let mut out = vec![0_u64; 16];
        let returned =
            control::<()>(&handle, FSCTL_QUERY_USN_JOURNAL, None, &mut out).map_err(|e| {
                match journal_error(&e) {
                    JournalError::Lost => JournalError::Failed(e.kind()),
                    other @ (JournalError::NotNtfs
                    | JournalError::NoJournal
                    | JournalError::Unsupported
                    | JournalError::Failed(_)) => other,
                }
            })?;
        let bytes = output(&out, returned)?;
        let bytes = bytes.as_slice();
        let short = JournalError::Failed(io::ErrorKind::UnexpectedEof);
        if bytes.len() < size_of::<USN_JOURNAL_DATA_V0>() {
            return Err(short);
        }
        let id = read_u64(bytes, offset_of!(USN_JOURNAL_DATA_V0, UsnJournalID)).ok_or(short)?;
        let first = read_i64(bytes, offset_of!(USN_JOURNAL_DATA_V0, FirstUsn)).ok_or(short)?;
        let next = read_i64(bytes, offset_of!(USN_JOURNAL_DATA_V0, NextUsn)).ok_or(short)?;
        let lowest =
            read_i64(bytes, offset_of!(USN_JOURNAL_DATA_V0, LowestValidUsn)).ok_or(short)?;
        Ok(JournalState {
            id,
            first: first.max(lowest),
            next,
        })
    }

    fn read(
        &self,
        root: &Path,
        journal: u64,
        from: Usn,
        to: Usn,
        each: &mut dyn FnMut(&Change) -> Flow,
    ) -> Result<(), JournalError> {
        let handle = open_root(root)?;
        let mut input = READ_USN_JOURNAL_DATA_V1 {
            StartUsn: from,
            ReasonMask: 0xFFFF_FFFF,
            ReturnOnlyOnClose: 0,
            Timeout: 0,
            BytesToWaitFor: 0,
            UsnJournalID: journal,
            MinMajorVersion: 2,
            MaxMajorVersion: 2,
        };
        let mut out = vec![0_u64; READ_WORDS];
        loop {
            let start = input.StartUsn;
            let returned = control(
                &handle,
                FSCTL_READ_UNPRIVILEGED_USN_JOURNAL,
                Some(&input),
                &mut out,
            )
            .map_err(|e| journal_error(&e))?;
            let bytes = output(&out, returned)?;
            let bytes = bytes.as_slice();
            let short = JournalError::Failed(io::ErrorKind::UnexpectedEof);
            let next = read_i64(bytes, 0).ok_or(short)?;
            let records = bytes.get(8..).ok_or(short)?;
            if records.is_empty() {
                // Nothing more to read: complete only if the journal's end is past `to`.
                return if next >= to { Ok(()) } else { Err(short) };
            }
            if hand_over(records, each)? == Flow::Stop || next >= to {
                return Ok(());
            }
            if next <= start {
                return Err(JournalError::Failed(io::ErrorKind::InvalidData));
            }
            input.StartUsn = next;
        }
    }

    fn path_of(&self, root: &Path, volume: u64, id: u64) -> io::Result<Option<Found>> {
        let mut full = [0_u8; 16];
        if let Some(low) = full.get_mut(..8) {
            low.copy_from_slice(&id.to_le_bytes());
        }
        let Some(object) = Object::open_by_id(root, FileId { volume, id: full }, 0)? else {
            return Ok(None);
        };
        Ok(Some(Found {
            path: PathBuf::from(OsString::from_wide(&object.normalized()?)),
            link: object.name_surrogate(),
        }))
    }

    fn tree(&self, root: &Path) -> io::Result<Vec<Listed>> {
        let top = Object::open_folder(root)?;
        if top.name_surrogate() || !top.folder {
            return Err(ApplyFailure::Changed.into());
        }
        let mut out = Vec::new();
        let mut folders = vec![top];
        while let Some(folder) = folders.pop() {
            let parent = own_id(&folder)?;
            for child in folder.children()? {
                if !child.folder {
                    out.push(Listed {
                        id: child.id,
                        parent,
                        folder: false,
                    });
                    continue;
                }
                let object = match folder.open_child(&child.name, true) {
                    Ok(object) => object,
                    // Gone since the listing: no name to record.
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    // A reparse the open refused to pass: not opened as itself, so not listed.
                    Err(e) if reparse_refused(&e) => return Err(ApplyFailure::Changed.into()),
                    Err(e) => return Err(e),
                };
                // Another folder under the name since the listing: the set would mix the two.
                if own_id(&object)? != child.id {
                    return Err(ApplyFailure::Changed.into());
                }
                if object.name_surrogate() {
                    out.push(Listed {
                        id: child.id,
                        parent,
                        folder: false,
                    });
                    continue;
                }
                if !object.folder {
                    return Err(ApplyFailure::Changed.into());
                }
                out.push(Listed {
                    id: child.id,
                    parent,
                    folder: true,
                });
                folders.push(object);
            }
        }
        Ok(out)
    }
}

/// Opens the own root `root` as itself for the journal's FSCTLs (any handle on the volume will
/// do; this one asks only to read attributes).
fn open_root(root: &Path) -> Result<Handle, JournalError> {
    volume::open_itself(root, volume::READ_ATTRIBUTES).map_err(|e| JournalError::Failed(e.kind()))
}

/// The 64-bit file reference of what `object` is open on; an error when its file ID doesn't fit.
fn own_id(object: &Object) -> io::Result<u64> {
    volume::short_id(object.file_id()?.1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "a file ID that doesn't fit in 64 bits",
        )
    })
}

/// `DeviceIoControl` with `code`, `input` (if any) and `out`; how many bytes it gave.
fn control<T>(handle: &Handle, code: u32, input: Option<&T>, out: &mut [u64]) -> io::Result<u32> {
    let (input_ptr, input_size) = match input {
        Some(input) => (
            Some(std::ptr::from_ref(input).cast()),
            u32::try_from(size_of::<T>()).map_err(io::Error::other)?,
        ),
        None => (None, 0),
    };
    let size = u32::try_from(size_of_val(out)).map_err(io::Error::other)?;
    let mut returned = 0_u32;
    // SAFETY: the handle is open and synchronous; `input`, when given, is a live `T` of the
    // size passed, which the call only reads; `out` is writable for `size` bytes and
    // `returned` is a live out-pointer; no overlapped structure.
    unsafe {
        DeviceIoControl(
            handle.0,
            code,
            input_ptr,
            input_size,
            Some(out.as_mut_ptr().cast()),
            size,
            Some(&raw mut returned),
            None,
        )
    }
    .map_err(|e| os_error(&e))?;
    Ok(returned)
}

/// The first `returned` bytes of `out`; an error when that is more than `out` holds.
fn output(out: &[u64], returned: u32) -> Result<Vec<u8>, JournalError> {
    let invalid = JournalError::Failed(io::ErrorKind::InvalidData);
    let returned =
        usize::try_from(returned).map_err(|e| JournalError::Failed(io::Error::other(e).kind()))?;
    if returned > size_of_val(out) {
        return Err(invalid);
    }
    Ok(out
        .iter()
        .flat_map(|word| word.to_ne_bytes())
        .take(returned)
        .collect())
}

/// What a journal call's error means.
fn journal_error(error: &io::Error) -> JournalError {
    match error.raw_os_error() {
        Some(JOURNAL_ENTRY_DELETED) => JournalError::Lost,
        Some(JOURNAL_NOT_ACTIVE | JOURNAL_DELETE_IN_PROGRESS) => JournalError::NoJournal,
        Some(INVALID_FUNCTION | NOT_SUPPORTED | INVALID_PARAMETER) => JournalError::Unsupported,
        _ => JournalError::Failed(error.kind()),
    }
}

/// Hands each `USN_RECORD_V2` in `records` (a read's output after its next USN) to `each`, in
/// order; whether `each` said stop. A record shorter than the fixed part, or past the end, is an
/// error; another major version is [`JournalError::Unsupported`].
fn hand_over(records: &[u8], each: &mut dyn FnMut(&Change) -> Flow) -> Result<Flow, JournalError> {
    let invalid = JournalError::Failed(io::ErrorKind::InvalidData);
    let fixed = offset_of!(USN_RECORD_V2, FileName);
    let mut at = 0_usize;
    while let Some(rest) = records.get(at..).filter(|rest| !rest.is_empty()) {
        let length = read_u32(rest, offset_of!(USN_RECORD_V2, RecordLength)).ok_or(invalid)?;
        let length = usize::try_from(length)
            .map_err(|e| JournalError::Failed(io::Error::other(e).kind()))?;
        if length < fixed {
            return Err(invalid);
        }
        let record = rest.get(..length).ok_or(invalid)?;
        if read_u16(record, offset_of!(USN_RECORD_V2, MajorVersion)) != Some(2) {
            return Err(JournalError::Unsupported);
        }
        let change = Change {
            usn: read_i64(record, offset_of!(USN_RECORD_V2, Usn)).ok_or(invalid)?,
            file: read_u64(record, offset_of!(USN_RECORD_V2, FileReferenceNumber))
                .ok_or(invalid)?,
            parent: read_u64(record, offset_of!(USN_RECORD_V2, ParentFileReferenceNumber))
                .ok_or(invalid)?,
            reason: read_u32(record, offset_of!(USN_RECORD_V2, Reason)).ok_or(invalid)?,
            attributes: read_u32(record, offset_of!(USN_RECORD_V2, FileAttributes))
                .ok_or(invalid)?,
        };
        if each(&change) == Flow::Stop {
            return Ok(Flow::Stop);
        }
        at += length;
    }
    Ok(Flow::More)
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

fn read_i64(bytes: &[u8], at: usize) -> Option<i64> {
    Some(i64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}
