//! An own root's facts for [`super::NtfsJournal`]: the root opened as itself by its path for
//! `FILE_READ_ATTRIBUTES` alone, its volume serial and file ID (`FileIdInfo`), its file system
//! (`GetVolumeInformationByHandleW`), its normalized final path, and whether its volume is on a
//! removable drive (`GetDriveTypeW`, then the storage bus from `IOCTL_STORAGE_QUERY_PROPERTY`).
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 file, volume and storage calls that read an own root's facts"
)]

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use windows::Win32::Storage::FileSystem::{
    BusType1394, BusTypeMmc, BusTypeSd, BusTypeUsb, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_ID_INFO, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileAttributeTagInfo, FileIdInfo,
    GETFINALPATHNAMEBYHANDLE_FLAGS, GetDriveTypeW, GetFileInformationByHandleEx,
    GetFinalPathNameByHandleW, GetVolumeInformationByHandleW, GetVolumeNameForVolumeMountPointW,
    GetVolumePathNameW, OPEN_EXISTING, SYNCHRONIZE, VOLUME_NAME_DOS,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery, STORAGE_DEVICE_DESCRIPTOR,
    STORAGE_PROPERTY_QUERY, StorageDeviceProperty,
};
use windows::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE};
use windows::core::PCWSTR;

use crate::windows::acl::object::{Handle, os_error};
use crate::windows::clean_close::RootFacts;
use crate::windows::clean_close::facts::{RootProbe, read_facts};
use crate::windows::folder::verbatim_wide;

/// `IsReparseTagNameSurrogate`: the tag's name-surrogate bit (junctions, symbolic links).
const NAME_SURROGATE: u32 = 0x2000_0000;

/// `MAX_PATH` and its NUL.
const MAX_PATH_WIDE: usize = 261;

/// What the root's handles ask for: its attributes, and nothing that could change it.
pub(super) const READ_ATTRIBUTES: u32 = FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;

/// The facts of the own root `path`, opened as itself, in [`read_facts`]' order: the file
/// system first, so a volume that isn't NTFS never meets an NTFS-only query.
pub(super) fn facts(path: &Path) -> io::Result<RootFacts> {
    let handle = open_itself(path, READ_ATTRIBUTES)?;
    read_facts(&Opened(handle))
}

/// An own root opened as itself, read through its handle.
struct Opened(Handle);

impl RootProbe for Opened {
    fn ntfs(&self) -> io::Result<bool> {
        is_ntfs(&self.0)
    }

    fn surrogate(&self) -> io::Result<bool> {
        name_surrogate(&self.0)
    }

    fn id(&self) -> io::Result<(u64, Option<u64>)> {
        let (volume, id) = id_info(&self.0)?;
        Ok((volume, short_id(id)))
    }

    fn final_path(&self) -> io::Result<PathBuf> {
        Ok(PathBuf::from(OsString::from_wide(&normalized(&self.0)?)))
    }

    fn removable(&self, path: &Path) -> bool {
        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        removable(&wide)
    }
}

/// The 64-bit file reference a 128-bit file ID holds: its low 8 bytes, little-endian, when
/// its high 8 bytes are zero (NTFS); `None` otherwise (`ReFS`).
pub(super) fn short_id(id: [u8; 16]) -> Option<u64> {
    let (low, high) = id.split_at(8);
    if high.iter().any(|&b| b != 0) {
        return None;
    }
    Some(u64::from_le_bytes(low.try_into().ok()?))
}

/// Opens `path` itself (`FILE_FLAG_OPEN_REPARSE_POINT`, so a link is opened, never followed;
/// `FILE_FLAG_BACKUP_SEMANTICS`, so a folder opens) for `access`.
pub(super) fn open_itself(path: &Path, access: u32) -> io::Result<Handle> {
    let wide = verbatim_wide(path)?;
    // SAFETY: `wide` is NUL-terminated and outlives the call; no security attributes or
    // template are passed, and the handle is owned by `Handle` from here.
    unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map(Handle)
    .map_err(|e| os_error(&e))
}

/// Whether what `handle` is open on is a junction, symbolic link or other name surrogate.
fn name_surrogate(handle: &Handle) -> io::Result<bool> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: the handle is open and `tag` is valid and writable for its size.
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileAttributeTagInfo,
            (&raw mut tag).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(0),
        )
    }
    .map_err(|e| os_error(&e))?;
    let reparse = tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
    Ok(reparse && tag.ReparseTag & NAME_SURROGATE != 0)
}

/// The volume serial number and 128-bit file ID of what `handle` is open on (`FileIdInfo`).
fn id_info(handle: &Handle) -> io::Result<(u64, [u8; 16])> {
    let mut info = FILE_ID_INFO::default();
    // SAFETY: the handle is open and `info` is valid and writable for its size.
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileIdInfo,
            (&raw mut info).cast(),
            u32::try_from(size_of::<FILE_ID_INFO>()).unwrap_or(0),
        )
    }
    .map_err(|e| os_error(&e))?;
    Ok((info.VolumeSerialNumber, info.FileId.Identifier))
}

/// Whether the file system of the volume `handle` is open on is NTFS, by its name.
pub(super) fn is_ntfs(handle: &Handle) -> io::Result<bool> {
    let mut name = [0_u16; MAX_PATH_WIDE];
    // SAFETY: the handle is open; `name` is a writable slice of its length, and the other
    // buffers are left out.
    unsafe { GetVolumeInformationByHandleW(handle.0, None, None, None, None, Some(&mut name)) }
        .map_err(|e| os_error(&e))?;
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Ok(name.get(..end) == Some("NTFS".encode_utf16().collect::<Vec<_>>().as_slice()))
}

/// The final path of what `handle` is open on, normalized, with its drive letter
/// (`\\?\C:\...`): the form `Object::normalized` gives, so `path_of`'s paths compare with it.
fn normalized(handle: &Handle) -> io::Result<Vec<u16>> {
    let flags = GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0);
    let mut buffer = vec![0_u16; 512];
    loop {
        // SAFETY: the handle is open and `buffer` is a writable slice of its length.
        let length = unsafe { GetFinalPathNameByHandleW(handle.0, &mut buffer, flags) };
        match usize::try_from(length).map_err(io::Error::other)? {
            0 => return Err(io::Error::last_os_error()),
            n if n < buffer.len() => {
                buffer.truncate(n);
                return Ok(buffer);
            }
            n => buffer.resize(n + 1, 0),
        }
    }
}

/// Whether the volume holding `path` (a final path, `\\?\C:\...`, no NUL) is on a drive another
/// computer could change unjournaled. Any step that fails answers yes: a removable volume
/// costs a full walk, while a removable volume taken for a fixed one could skip a walk it needs.
fn removable(path: &[u16]) -> bool {
    let Some(volume) = volume_name(path) else {
        return true;
    };
    // SAFETY: `volume` is NUL-terminated UTF-16 that outlives the call.
    let drive = unsafe { GetDriveTypeW(PCWSTR(volume.as_ptr())) };
    if drive == DRIVE_REMOVABLE {
        return true;
    }
    // Not a fixed drive (remote, optical, RAM, or unknown): not a volume this record trusts.
    if drive != DRIVE_FIXED {
        return true;
    }
    // A USB or FireWire disk, or a card, reads as a fixed drive; its bus says what it is. If the
    // bus can't be read (the call refused at this token, a volume over several disks), the
    // answer is removable: the caller then does the full walk rather than trust a volume that
    // may have been changed elsewhere.
    match bus_type(&volume) {
        Some(bus) => [BusTypeUsb, BusType1394, BusTypeSd, BusTypeMmc]
            .iter()
            .any(|kind| kind.0 == bus),
        None => true,
    }
}

/// The volume GUID path (`\\?\Volume{...}\`, NUL-terminated) of the volume holding `path`:
/// its volume root (`GetVolumePathNameW`, without the `\\?\` the final path has, as
/// `GetVolumeNameForVolumeMountPointW` wants a drive or a mounted folder), then its name.
fn volume_name(path: &[u16]) -> Option<Vec<u16>> {
    let mut wide = path.to_vec();
    wide.push(0);
    let mut root = vec![0_u16; wide.len().max(MAX_PATH_WIDE)];
    // SAFETY: `wide` is NUL-terminated and outlives the call; `root` is writable for its length.
    if let Err(error) = unsafe { GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut root) } {
        tracing::debug!(code = error.code().0, "GetVolumePathNameW failed");
        return None;
    }
    let end = root.iter().position(|&c| c == 0)?;
    root.truncate(end);
    let verbatim: Vec<u16> = r"\\?\".encode_utf16().collect();
    let unc: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
    if root.starts_with(&unc) {
        return None;
    }
    let mut mount = root
        .strip_prefix(verbatim.as_slice())
        .unwrap_or(&root)
        .to_vec();
    mount.push(0);
    let mut name = vec![0_u16; 64];
    // SAFETY: `mount` is NUL-terminated and outlives the call; `name` is writable for its
    // length (a volume GUID path is 49 characters and its NUL).
    let named = unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(mount.as_ptr()), &mut name) };
    if let Err(error) = named {
        tracing::debug!(
            code = error.code().0,
            "GetVolumeNameForVolumeMountPointW failed"
        );
        return None;
    }
    let end = name.iter().position(|&c| c == 0)?;
    name.truncate(end + 1);
    Some(name)
}

/// The storage bus of the volume `volume` (a volume GUID path with its `\` and NUL): the
/// volume's device opened with no access rights, which any user may do, and asked for its
/// `StorageDeviceProperty`. `None` when either call fails.
fn bus_type(volume: &[u16]) -> Option<i32> {
    // The device is the GUID path without its trailing `\` (with it, the root folder opens).
    let mut device: Vec<u16> = volume.iter().copied().take_while(|&c| c != 0).collect();
    if device.last() == Some(&u16::from(b'\\')) {
        device.pop();
    }
    device.push(0);
    // SAFETY: `device` is NUL-terminated and outlives the call; no security attributes or
    // template are passed, and the handle is owned by `Handle` from here.
    let opened = unsafe {
        CreateFileW(
            PCWSTR(device.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    };
    let handle = match opened {
        Ok(handle) => Handle(handle),
        Err(error) => {
            tracing::debug!(code = error.code().0, "opening the volume's device failed");
            return None;
        }
    };
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        ..STORAGE_PROPERTY_QUERY::default()
    };
    let mut out = vec![0_u64; 128];
    let size = u32::try_from(out.len() * 8).ok()?;
    let mut returned = 0_u32;
    // SAFETY: the handle is open; `query` is a live `STORAGE_PROPERTY_QUERY` of the size given;
    // `out` is writable for `size` bytes and `returned` is a live out-pointer; no overlap.
    let asked = unsafe {
        DeviceIoControl(
            handle.0,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some((&raw const query).cast()),
            u32::try_from(size_of::<STORAGE_PROPERTY_QUERY>()).ok()?,
            Some(out.as_mut_ptr().cast()),
            size,
            Some(&raw mut returned),
            None,
        )
    };
    if let Err(error) = asked {
        tracing::debug!(code = error.code().0, "IOCTL_STORAGE_QUERY_PROPERTY failed");
        return None;
    }
    let bytes: Vec<u8> = out.iter().flat_map(|w| w.to_ne_bytes()).collect();
    let bytes = bytes.get(..usize::try_from(returned).ok()?)?;
    let at = std::mem::offset_of!(STORAGE_DEVICE_DESCRIPTOR, BusType);
    Some(i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
