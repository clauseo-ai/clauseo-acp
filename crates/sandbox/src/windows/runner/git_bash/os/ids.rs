//! [`FileIds`] on Windows, as the desktop user: a drive's type and a folder's identity
//! (`FileIdInfo`) through a handle opened for its attributes alone, as itself or through its
//! links. A network drive is told by its type, which asks nothing of the network.
#![expect(
    unsafe_code,
    reason = "FFI to GetDriveTypeW and GetFileInformationByHandleEx"
)]

use std::fs::File;
use std::io;
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::AsRawHandle as _;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_INFO_BY_HANDLE_CLASS, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileAttributeTagInfo, FileIdInfo,
    GetDriveTypeW, GetFileInformationByHandleEx,
};
use windows::core::HSTRING;

use super::super::path_ids::{FileId, FileIds};

/// `DRIVE_REMOTE`.
const DRIVE_REMOTE: u32 = 4;

/// The machine.
pub(super) struct WindowsIds;

impl FileIds for WindowsIds {
    fn remote(&mut self, drive: &str) -> bool {
        let root = HSTRING::from(format!(r"{drive}\"));
        // SAFETY: `root` is NUL-terminated and outlives the call.
        unsafe { GetDriveTypeW(&root) == DRIVE_REMOTE }
    }

    fn plain_id(&mut self, path: &str) -> Option<(FileId, bool)> {
        let folder = open_folder(&format!(r"\\?\{path}"), false).ok()?;
        let tag: FILE_ATTRIBUTE_TAG_INFO = info(&folder, FileAttributeTagInfo).ok()?;
        let id = id_of(&folder).ok()?;
        Some((id, tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0))
    }

    fn root_id(&mut self, path: &str) -> Option<FileId> {
        id_of(&open_folder(path, true).ok()?).ok()
    }
}

/// The folder (or file) `path` opened for its attributes alone, sharing everything; as itself
/// unless `follow`.
#[expect(
    clippy::disallowed_methods,
    reason = "opens a folder for its attributes, never to write; std's open is the one with flags"
)]
fn open_folder(path: &str, follow: bool) -> io::Result<File> {
    let mut flags = FILE_FLAG_BACKUP_SEMANTICS.0;
    if !follow {
        flags |= FILE_FLAG_OPEN_REPARSE_POINT.0;
    }
    File::options()
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .custom_flags(flags)
        .open(path)
}

/// What `file` is open on: its volume's serial number and 128-bit file ID.
fn id_of(file: &File) -> io::Result<FileId> {
    let found: FILE_ID_INFO = info(file, FileIdInfo)?;
    Ok((
        found.VolumeSerialNumber,
        u128::from_le_bytes(found.FileId.Identifier),
    ))
}

/// One `GetFileInformationByHandleEx` class read into `T`.
fn info<T: Default>(file: &File, class: FILE_INFO_BY_HANDLE_CLASS) -> io::Result<T> {
    let mut out = T::default();
    let size = u32::try_from(size_of::<T>()).map_err(io::Error::other)?;
    // SAFETY: the handle is `file`'s, open for the call; `out` is a live, writable `T` of
    // `size` bytes, the layout the class names.
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            class,
            (&raw mut out).cast(),
            size,
        )
    }
    .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF))?;
    Ok(out)
}
