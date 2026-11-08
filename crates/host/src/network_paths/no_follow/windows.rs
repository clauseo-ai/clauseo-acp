//! The no-follow open on Windows: `NtCreateFile` one name at a time, relative to the folder's
//! handle, from the volume's device. Each name is read from its folder's listing first
//! (`NtQueryDirectoryFile` on a fresh handle to the folder, which opens nothing): a link (a
//! symlink, a junction, any name-surrogate tag) refuses, and so does a reparse point whose tag
//! is not a known local kind ([`local_reparse_tag`]). The open takes `FILE_OPEN_REPARSE_POINT`
//! (a reparse point is opened as itself, never followed) and its attributes are read again
//! from the handle, so a swap between the listing and the open refuses too.
#![expect(
    unsafe_code,
    reason = "FFI to NtCreateFile, NtQueryDirectoryFile, DeviceIoControl, \
              GetFileInformationByHandleEx and GetFinalPathNameByHandleW for the no-follow open"
)]

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::{Component, Path, PathBuf, Prefix};

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
    FILE_SYNCHRONOUS_IO_NONALERT, FileFullDirectoryInformation, NTCREATEFILE_CREATE_OPTIONS,
    NtCreateFile, NtQueryDirectoryFile,
};
use windows::Win32::Foundation::{
    HANDLE, NTSTATUS, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, STATUS_NO_MORE_FILES,
    STATUS_NO_SUCH_FILE, UNICODE_STRING,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ACCESS_RIGHTS, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAGS_AND_ATTRIBUTES, FILE_GENERIC_READ,
    FILE_ID_INFO, FILE_LIST_DIRECTORY, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
    FileAttributeTagInfo, FileIdInfo, GETFINALPATHNAMEBYHANDLE_FLAGS, GetFileInformationByHandleEx,
    GetFinalPathNameByHandleW, MAXIMUM_REPARSE_DATA_BUFFER_SIZE, SYNCHRONIZE, VOLUME_NAME_DOS,
};
use windows::Win32::System::IO::{DeviceIoControl, IO_STATUS_BLOCK};
use windows::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
use windows::core::PWSTR;

use super::listing::{DIRECTORY, EntryInfo, REPARSE_POINT, dir_entry, link_target, name_surrogate};
use super::{Access, Kind, link_on_the_way};
use crate::network_paths::resolve::Entry;
use crate::network_paths::windows::device_of;
use crate::network_paths::windows_names::{local_reparse_tag, volume_of};

/// An open handle, reached with no link followed.
#[derive(Debug)]
pub(super) struct Handle(OwnedHandle);

/// Room for one listing entry: the fixed part and a name of up to 32 767 UTF-16 units.
const ENTRY_ROOM: usize = 68 + 2 * 32_768;

/// How a reparse point at the last name is taken.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    /// A link refuses ([`link_on_the_way`]).
    Refuse,
    /// A link is opened as itself (to read its target or its identity).
    Itself,
}

/// What `access` asks of the handle, and the create options it needs.
fn rights(
    access: Access,
) -> (
    FILE_ACCESS_RIGHTS,
    FILE_SHARE_MODE,
    NTCREATEFILE_CREATE_OPTIONS,
) {
    let all = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    match access {
        Access::Look => (FILE_READ_ATTRIBUTES, all, NTCREATEFILE_CREATE_OPTIONS(0)),
        Access::Folder | Access::List => (
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES,
            all,
            FILE_DIRECTORY_FILE,
        ),
        Access::Read => (FILE_GENERIC_READ, all, FILE_NON_DIRECTORY_FILE),
        Access::Exclusive => (
            FILE_GENERIC_READ,
            FILE_SHARE_MODE(0),
            FILE_NON_DIRECTORY_FILE,
        ),
    }
}

/// An `io::Error` for an `NTSTATUS`, as the Win32 code it maps to (so `NotFound` and the sharing
/// violation read as they do from `CreateFileW`).
fn nt_error(status: NTSTATUS) -> io::Error {
    // SAFETY: a pure lookup of the status code; reads only the thread's own block.
    let code = unsafe { RtlNtStatusToDosError(status) };
    io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
}

/// An `io::Error` for a failed Win32 call, as its Win32 code.
fn win_error(error: &windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(error.code().0 & 0xFFFF)
}

/// The length in bytes of the UTF-16 `name`, as a `UNICODE_STRING` holds it.
fn byte_len(name: &[u16]) -> io::Result<u16> {
    u16::try_from(name.len().saturating_mul(2))
        .map_err(|_too_long| io::Error::new(io::ErrorKind::InvalidFilename, "name too long"))
}

/// `NtCreateFile` of `name` (UTF-16, no NUL) relative to `root` (absolute when `None`), for
/// `access` and `share`, with `FILE_OPEN_REPARSE_POINT` always: a reparse point is opened as
/// itself, never followed.
fn nt_open(
    root: Option<&OwnedHandle>,
    name: &[u16],
    access: FILE_ACCESS_RIGHTS,
    share: FILE_SHARE_MODE,
    options: NTCREATEFILE_CREATE_OPTIONS,
) -> io::Result<OwnedHandle> {
    let bytes = byte_len(name)?;
    let object_name = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(name.as_ptr().cast_mut()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(0),
        RootDirectory: root.map_or(HANDLE::default(), |root| HANDLE(root.as_raw_handle())),
        ObjectName: &raw const object_name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut handle = HANDLE::default();
    let mut status_block = IO_STATUS_BLOCK::default();
    // SAFETY: every pointer is to a local that outlives the call (`name` backs `object_name`,
    // which `attributes` points to); `root`, when given, is an open handle; the call writes only
    // `handle` and `status_block`. FILE_OPEN never creates anything.
    let status = unsafe {
        NtCreateFile(
            &raw mut handle,
            access | SYNCHRONIZE,
            &raw const attributes,
            &raw mut status_block,
            None,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            share,
            FILE_OPEN,
            options | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            None,
            0,
        )
    };
    if status.is_err() {
        return Err(nt_error(status));
    }
    // SAFETY: the call succeeded, so `handle` is a new open handle this function now owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

/// `name` as UTF-16, refused when the listing would read it as a pattern (`*`, `?`, `<`, `>`,
/// `"`: no file has such a name) or it holds a separator.
fn wide_name(name: &OsStr) -> io::Result<Vec<u16>> {
    let wide: Vec<u16> = name.encode_wide().collect();
    let pattern = (*b"*?<>\"\\/").map(u16::from);
    if wide.is_empty() || wide.iter().any(|unit| pattern.contains(unit)) {
        // ERROR_INVALID_NAME, which the walk takes as a name that can't be there.
        return Err(io::Error::from_raw_os_error(123));
    }
    Ok(wide)
}

/// Opens the absolute `path` for `access`, from its volume's device, no name followed.
pub(super) fn open_path(path: &Path, access: Access) -> io::Result<Handle> {
    let mut parts = path.components();
    let letter = match parts.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => char::from(letter),
            Prefix::Verbatim(_)
            | Prefix::VerbatimUNC(..)
            | Prefix::DeviceNS(_)
            | Prefix::UNC(..) => return Err(not_local()),
        },
        _ => return Err(not_local()),
    };
    if parts.next() != Some(Component::RootDir) {
        return Err(not_local());
    }
    let (device, subst) = volume_of(letter, &device_of).ok_or_else(not_local)?;
    let mut names: Vec<OsString> = subst.into_iter().map(OsString::from).collect();
    for part in parts {
        match part {
            Component::Normal(name) => names.push(name.to_os_string()),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not an absolute path of plain names",
                ));
            }
        }
    }
    let root: Vec<u16> = format!("{device}\\").encode_utf16().collect();
    let Some((last, folders)) = names.split_last() else {
        let (rights, share, options) = rights(access);
        let opened = Handle(nt_open(None, &root, rights, share, options)?);
        opened.refuse_reparse(Last::Refuse)?;
        return Ok(opened);
    };
    let (list, share, folder) = rights(Access::List);
    let mut at = Handle(nt_open(None, &root, list, share, folder)?);
    at.refuse_reparse(Last::Refuse)?;
    for name in folders {
        at = at.open_child(name, Access::List, Last::Refuse)?;
    }
    at.open_child(last, access, Last::Refuse)
}

/// The error for a path not on a local drive.
fn not_local() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "not a path on a local drive")
}

impl Handle {
    /// The listing entry `name` of this folder, read on a fresh handle to it (a handle's
    /// listing keeps the pattern of its first query): nothing opened but the folder itself.
    fn listed(&self, name: &OsStr) -> io::Result<EntryInfo> {
        match self.listed_by_query(name) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                self.attributes_by_open(name)
            }
            other => other,
        }
    }

    /// [`Self::listed`] for a folder this user may pass through but not list (only a stat
    /// needed that before): the entry opened as itself (`FILE_OPEN_REPARSE_POINT`, attributes
    /// only, never followed) and its attributes and tag read from that handle. A local path under
    /// such a folder stays usable.
    fn attributes_by_open(&self, name: &OsStr) -> io::Result<EntryInfo> {
        let wide = wide_name(name)?;
        let (rights, share, options) = rights(Access::Look);
        let opened = nt_open(Some(&self.0), &wide, rights, share, options)?;
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        let size = u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(0);
        // SAFETY: `opened` is open; `info` is a FILE_ATTRIBUTE_TAG_INFO of `size` bytes.
        unsafe {
            GetFileInformationByHandleEx(
                HANDLE(opened.as_raw_handle()),
                FileAttributeTagInfo,
                (&raw mut info).cast(),
                size,
            )
        }
        .map_err(|error| win_error(&error))?;
        Ok(EntryInfo {
            attributes: info.FileAttributes,
            tag: if info.FileAttributes & REPARSE_POINT == 0 {
                0
            } else {
                info.ReparseTag
            },
            name: wide,
        })
    }

    /// The entry `name` from this folder's listing, filtered to that one name.
    fn listed_by_query(&self, name: &OsStr) -> io::Result<EntryInfo> {
        let wide = wide_name(name)?;
        let (list, share, folder) = rights(Access::List);
        let fresh = nt_open(Some(&self.0), &[], list, share, folder)?;
        let pattern_bytes = byte_len(&wide)?;
        let pattern = UNICODE_STRING {
            Length: pattern_bytes,
            MaximumLength: pattern_bytes,
            Buffer: PWSTR(wide.as_ptr().cast_mut()),
        };
        let mut room = vec![0_u64; ENTRY_ROOM.div_ceil(8)];
        let room_bytes = u32::try_from(room.len().saturating_mul(8)).unwrap_or(0);
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: `fresh` is an open folder handle; `room` is a writable buffer of `room_bytes`
        // bytes, 8-aligned; `pattern` points into `wide`, both alive across the call.
        let status = unsafe {
            NtQueryDirectoryFile(
                HANDLE(fresh.as_raw_handle()),
                None,
                None,
                None,
                &raw mut status_block,
                room.as_mut_ptr().cast(),
                room_bytes,
                FileFullDirectoryInformation,
                true,
                Some(&raw const pattern),
                true,
            )
        };
        if status == STATUS_NO_SUCH_FILE || status == STATUS_NO_MORE_FILES {
            return Err(io::ErrorKind::NotFound.into());
        }
        if status.is_err() {
            return Err(nt_error(status));
        }
        let bytes: Vec<u8> = room.iter().flat_map(|word| word.to_ne_bytes()).collect();
        dir_entry(&bytes)
            .map(|(info, _)| info)
            .ok_or_else(|| io::Error::other("a listing entry that can't be read"))
    }

    /// An error when this handle is a reparse point it may not be: a link unless `last` takes
    /// one as itself, or a tag not known to be local.
    fn refuse_reparse(&self, last: Last) -> io::Result<()> {
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        let size = u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(0);
        // SAFETY: the handle is open; `info` is a FILE_ATTRIBUTE_TAG_INFO of `size` bytes.
        unsafe {
            GetFileInformationByHandleEx(
                HANDLE(self.0.as_raw_handle()),
                FileAttributeTagInfo,
                (&raw mut info).cast(),
                size,
            )
        }
        .map_err(|error| win_error(&error))?;
        reparse_allowed(info.FileAttributes, info.ReparseTag, last)
    }

    /// The entry `name` of this folder opened for `access`, read from the listing first and
    /// its attributes checked again on the handle.
    fn open_child(&self, name: &OsStr, access: Access, last: Last) -> io::Result<Self> {
        let info = self.listed(name)?;
        reparse_allowed(info.attributes, info.tag, last)?;
        let wide = wide_name(name)?;
        let (rights, share, options) = rights(access);
        let opened = Self(nt_open(Some(&self.0), &wide, rights, share, options)?);
        opened.refuse_reparse(last)?;
        Ok(opened)
    }

    pub(super) fn kind(&self) -> io::Result<Kind> {
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        let size = u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(0);
        // SAFETY: the handle is open; `info` is a FILE_ATTRIBUTE_TAG_INFO of `size` bytes.
        unsafe {
            GetFileInformationByHandleEx(
                HANDLE(self.0.as_raw_handle()),
                FileAttributeTagInfo,
                (&raw mut info).cast(),
                size,
            )
        }
        .map_err(|error| win_error(&error))?;
        Ok(if info.FileAttributes & DIRECTORY != 0 {
            Kind::Dir
        } else {
            Kind::File
        })
    }

    pub(super) fn identity(&self) -> io::Result<(u64, u128)> {
        let mut info = FILE_ID_INFO::default();
        let size = u32::try_from(size_of::<FILE_ID_INFO>()).unwrap_or(0);
        // SAFETY: the handle is open; `info` is a FILE_ID_INFO of `size` bytes written in place.
        unsafe {
            GetFileInformationByHandleEx(
                HANDLE(self.0.as_raw_handle()),
                FileIdInfo,
                (&raw mut info).cast(),
                size,
            )
        }
        .map_err(|error| win_error(&error))?;
        Ok((
            info.VolumeSerialNumber,
            u128::from_le_bytes(info.FileId.Identifier),
        ))
    }

    pub(super) fn entry_at(&self, name: &OsStr) -> io::Result<Entry> {
        listed_entry(&self.listed(name)?)
    }

    pub(super) fn read_link_at(&self, name: &OsStr) -> io::Result<PathBuf> {
        let link = self.open_child(name, Access::Look, Last::Itself)?;
        let mut room = vec![0_u8; usize::try_from(MAXIMUM_REPARSE_DATA_BUFFER_SIZE).unwrap_or(0)];
        let room_bytes = u32::try_from(room.len()).unwrap_or(0);
        let mut written = 0_u32;
        // SAFETY: `link` is an open handle; `room` is writable for `room_bytes` bytes and
        // `written` is a local; the call is synchronous (no OVERLAPPED).
        unsafe {
            DeviceIoControl(
                HANDLE(link.0.as_raw_handle()),
                FSCTL_GET_REPARSE_POINT,
                None,
                0,
                Some(room.as_mut_ptr().cast()),
                room_bytes,
                Some(&raw mut written),
                None,
            )
        }
        .map_err(|error| win_error(&error))?;
        let filled = room
            .get(..usize::try_from(written).unwrap_or(0))
            .unwrap_or_default();
        let target = link_target(filled)
            .ok_or_else(|| io::Error::other("a reparse point that is not a link"))?;
        Ok(PathBuf::from(OsString::from_wide(&target)))
    }

    pub(super) fn identity_at(&self, name: &OsStr) -> io::Result<(u64, u128)> {
        self.open_child(name, Access::Look, Last::Itself)?
            .identity()
    }

    pub(super) fn open_at(&self, name: &OsStr, access: Access) -> io::Result<Self> {
        self.open_child(name, access, Last::Refuse)
    }

    pub(super) fn entries(&self, max: usize) -> io::Result<Vec<(OsString, Option<Entry>)>> {
        let (list, share, folder) = rights(Access::List);
        let fresh = nt_open(Some(&self.0), &[], list, share, folder)?;
        let mut room = vec![0_u64; 8192];
        let room_bytes = u32::try_from(room.len().saturating_mul(8)).unwrap_or(0);
        let mut names = Vec::new();
        let mut first = true;
        while names.len() < max {
            let mut status_block = IO_STATUS_BLOCK::default();
            // SAFETY: `fresh` is an open folder handle; `room` is a writable, 8-aligned buffer
            // of `room_bytes` bytes; no pattern is given.
            let status = unsafe {
                NtQueryDirectoryFile(
                    HANDLE(fresh.as_raw_handle()),
                    None,
                    None,
                    None,
                    &raw mut status_block,
                    room.as_mut_ptr().cast(),
                    room_bytes,
                    FileFullDirectoryInformation,
                    false,
                    None,
                    first,
                )
            };
            first = false;
            if status == STATUS_NO_MORE_FILES {
                break;
            }
            if status.is_err() {
                return Err(nt_error(status));
            }
            let bytes: Vec<u8> = room.iter().flat_map(|word| word.to_ne_bytes()).collect();
            let mut at = 0_usize;
            while let Some((info, next)) = bytes.get(at..).and_then(dir_entry) {
                let name = OsString::from_wide(&info.name);
                if name != "." && name != ".." && names.len() < max {
                    names.push((name, listed_entry(&info).ok()));
                }
                if next == 0 {
                    break;
                }
                at = at.saturating_add(next);
            }
        }
        Ok(names)
    }

    pub(super) fn final_path(&self) -> io::Result<PathBuf> {
        let mut room = vec![0_u16; 1024];
        for _ in 0..2 {
            // SAFETY: the handle is open; `room` is written in place, at most its length.
            let written = unsafe {
                GetFinalPathNameByHandleW(
                    HANDLE(self.0.as_raw_handle()),
                    &mut room,
                    GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0),
                )
            };
            let written = usize::try_from(written).unwrap_or(0);
            if written == 0 {
                return Err(io::Error::last_os_error());
            }
            if let Some(path) = room.get(..written) {
                let path = PathBuf::from(OsString::from_wide(path));
                return match path.components().next() {
                    Some(Component::Prefix(prefix))
                        if matches!(prefix.kind(), Prefix::VerbatimDisk(_) | Prefix::Disk(_)) =>
                    {
                        Ok(dunce::simplified(&path).to_path_buf())
                    }
                    _ => Err(not_local()),
                };
            }
            room.resize(written.saturating_add(1), 0);
        }
        Err(io::Error::other("the final path kept growing"))
    }

    pub(super) fn into_file(self) -> File {
        File::from(self.0)
    }

    #[expect(
        clippy::unused_self,
        reason = "the same signature as macOS's, which reads the folder's volume"
    )]
    pub(super) fn local_apfs_or_hfs(&self) -> Option<bool> {
        None
    }
}

/// What a listing entry is: a link for a name-surrogate tag, an error for a reparse point not
/// known to be local (never opened), else a folder or a file.
fn listed_entry(info: &EntryInfo) -> io::Result<Entry> {
    if info.attributes & REPARSE_POINT != 0 {
        if name_surrogate(info.tag) {
            return Ok(Entry::Link);
        }
        if !local_reparse_tag(info.tag) {
            return Err(io::Error::other(
                "a reparse point of a kind not known to be local",
            ));
        }
    }
    Ok(if info.attributes & DIRECTORY != 0 {
        Entry::Dir
    } else {
        Entry::File
    })
}

/// An error when attributes `attributes` with reparse tag `tag` may not be on a walked path: a
/// link (unless `last` takes one as itself), or a reparse point not known to be local.
fn reparse_allowed(attributes: u32, tag: u32, last: Last) -> io::Result<()> {
    if attributes & REPARSE_POINT == 0 {
        return Ok(());
    }
    if name_surrogate(tag) {
        return if last == Last::Itself {
            Ok(())
        } else {
            Err(link_on_the_way())
        };
    }
    if local_reparse_tag(tag) {
        Ok(())
    } else {
        Err(link_on_the_way())
    }
}
