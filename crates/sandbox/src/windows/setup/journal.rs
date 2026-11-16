//! The change journal at set-up on Windows (S15 `PR4f`, step 9, D-B12): which volumes, their
//! journal's size (`FSCTL_QUERY_USN_JOURNAL`), and the create or enlarge
//! (`FSCTL_CREATE_USN_JOURNAL`). The plan is `journal_plan.rs` (pure); this file only asks and
//! acts. Every volume is opened by its volume GUID path, never by a path a user could redirect.
#![expect(unsafe_code, reason = "FFI to the volume calls and DeviceIoControl")]

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_JOURNAL_NOT_ACTIVE, GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    GetVolumeNameForVolumeMountPointW, GetVolumePathNameW, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    CREATE_USN_JOURNAL_DATA, FSCTL_CREATE_USN_JOURNAL, FSCTL_QUERY_USN_JOURNAL, USN_JOURNAL_DATA_V0,
};
use windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows::core::PCWSTR;

use super::ffi::{PROFILE_LIST, os, reg_string, wide};
use super::journal_plan::{VolumeFacts, device_path, grows, select_volumes};
use super::machine::MachineError;
use crate::windows::acl::object::Handle;
use crate::windows::clean_close::{Journal as _, NtfsJournal};

/// `MAX_PATH` and its NUL.
const MAX_PATH_WIDE: usize = 261;

/// The volumes whose journal set-up enlarges (D-B12): those holding any of `roots`, the
/// Windows folder (`GetSystemWindowsDirectoryW`, never the `SystemDrive` variable, which the
/// process that started the set-up could set) and the profiles' folder, kept only when NTFS
/// and fixed ([`select_volumes`]). A root whose volume can't be read is passed over.
pub fn journal_volumes(roots: &[&Path]) -> Result<Vec<PathBuf>, MachineError> {
    let mut paths: Vec<PathBuf> = roots.iter().map(|root| root.to_path_buf()).collect();
    paths.push(windows_folder()?);
    if let Some(profiles) = reg_string(PROFILE_LIST, "ProfilesDirectory")? {
        paths.push(PathBuf::from(profiles));
    }
    let mut candidates = Vec::new();
    for path in paths {
        let Some(root) = volume_root(&path) else {
            continue;
        };
        match NtfsJournal.root(&root) {
            Ok(facts) => candidates.push(VolumeFacts {
                root,
                ntfs: facts.ntfs,
                removable: facts.removable,
            }),
            Err(error) => tracing::debug!(%error, "a volume's facts couldn't be read"),
        }
    }
    Ok(select_volumes(candidates))
}

/// The change journal's `MaximumSize` on the volume whose root is `volume`; `None` when it has
/// no journal (`ERROR_JOURNAL_NOT_ACTIVE`).
pub fn journal_size(volume: &Path) -> Result<Option<u64>, MachineError> {
    let handle = open_volume(volume, GENERIC_READ.0)?;
    let mut data = USN_JOURNAL_DATA_V0::default();
    let size = u32::try_from(size_of::<USN_JOURNAL_DATA_V0>()).unwrap_or(0);
    let mut returned = 0u32;
    // SAFETY: the volume handle is open and synchronous; no input; `data` is writable for
    // `size` bytes and `returned` is a live out-pointer; no overlapped structure.
    let queried = unsafe {
        DeviceIoControl(
            handle.0,
            FSCTL_QUERY_USN_JOURNAL,
            None,
            0,
            Some((&raw mut data).cast()),
            size,
            Some(&raw mut returned),
            None,
        )
    };
    match queried {
        Ok(()) => Ok(Some(data.MaximumSize)),
        Err(error) if error.code() == ERROR_JOURNAL_NOT_ACTIVE.to_hresult() => Ok(None),
        Err(error) => Err(os("FSCTL_QUERY_USN_JOURNAL", &error)),
    }
}

/// Creates or enlarges the change journal of the volume whose root is `volume` to `max` bytes,
/// growing by `delta`. Refuses a volume that isn't fixed and NTFS. Asks the journal's size
/// first and does nothing when it is `max` or larger ([`grows`]): a journal is never shrunk.
pub fn grow_journal(volume: &Path, max: u64, delta: u64) -> Result<(), MachineError> {
    // Checked again here, so no caller can grow a journal on a removable or non-NTFS volume.
    let facts = NtfsJournal
        .root(volume)
        .map_err(|error| MachineError::Other(format!("the volume can't be read: {error}")))?;
    if !facts.ntfs || facts.removable {
        return Err(MachineError::Other(
            "the volume isn't a fixed NTFS volume".to_owned(),
        ));
    }
    if !grows(journal_size(volume)?, max) {
        return Ok(());
    }
    let handle = open_volume(volume, GENERIC_READ.0 | GENERIC_WRITE.0)?;
    let input = CREATE_USN_JOURNAL_DATA {
        MaximumSize: max,
        AllocationDelta: delta,
    };
    let size = u32::try_from(size_of::<CREATE_USN_JOURNAL_DATA>()).unwrap_or(0);
    let mut returned = 0u32;
    // SAFETY: the volume handle is open for writing and synchronous; `input` is live and `size`
    // bytes long, and the call only reads it; no output; no overlapped structure.
    unsafe {
        DeviceIoControl(
            handle.0,
            FSCTL_CREATE_USN_JOURNAL,
            Some(std::ptr::from_ref(&input).cast()),
            size,
            None,
            0,
            Some(&raw mut returned),
            None,
        )
    }
    .map_err(|error| os("FSCTL_CREATE_USN_JOURNAL", &error))
}

/// The root of the volume holding `path` (`GetVolumePathNameW`: `C:\`, or a mounted folder,
/// with its trailing backslash); `None` when it can't be read.
fn volume_root(path: &Path) -> Option<PathBuf> {
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut root = vec![0u16; wide_path.len().max(MAX_PATH_WIDE)];
    // SAFETY: `wide_path` is NUL-terminated and outlives the call; `root` is writable for its
    // length.
    if let Err(error) = unsafe { GetVolumePathNameW(PCWSTR(wide_path.as_ptr()), &mut root) } {
        tracing::debug!(code = error.code().0, "GetVolumePathNameW failed");
        return None;
    }
    let end = root.iter().position(|&unit| unit == 0)?;
    root.truncate(end);
    Some(PathBuf::from(OsString::from_wide(&root)))
}

/// The Windows folder (`C:\Windows`), as Windows reports it.
fn windows_folder() -> Result<PathBuf, MachineError> {
    let mut buffer = vec![0u16; MAX_PATH_WIDE];
    // SAFETY: `buffer` is writable for its length, which the call is given with it.
    let written = unsafe { GetSystemWindowsDirectoryW(Some(&mut buffer)) };
    let written = usize::try_from(written).unwrap_or(usize::MAX);
    if written == 0 || written >= buffer.len() {
        return Err(MachineError::Os {
            call: "GetSystemWindowsDirectoryW",
            code: std::io::Error::last_os_error()
                .raw_os_error()
                .and_then(|code| u32::try_from(code).ok())
                .unwrap_or(u32::MAX),
        });
    }
    buffer.truncate(written);
    Ok(PathBuf::from(OsString::from_wide(&buffer)))
}

/// Opens the volume whose root is `volume` for `access`, by its volume GUID path
/// (`GetVolumeNameForVolumeMountPointW`, then [`device_path`]).
fn open_volume(volume: &Path, access: u32) -> Result<Handle, MachineError> {
    let mut mount: Vec<u16> = volume.as_os_str().encode_wide().collect();
    if mount.last() != Some(&u16::from(b'\\')) {
        mount.push(u16::from(b'\\'));
    }
    mount.push(0);
    let mut name = vec![0u16; 64];
    // SAFETY: `mount` is NUL-terminated and outlives the call; `name` is writable for its
    // length (a volume GUID path is 49 characters and its NUL).
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(mount.as_ptr()), &mut name) }
        .map_err(|error| os("GetVolumeNameForVolumeMountPointW", &error))?;
    let end = name
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(name.len());
    name.truncate(end);
    let name = OsString::from_wide(&name);
    let device = device_path(&name.to_string_lossy())
        .ok_or_else(|| MachineError::Other("a volume name isn't a volume GUID path".into()))?;
    let device = wide(&device);
    // SAFETY: `device` is NUL-terminated and outlives the call; no security attributes or
    // template; the handle is owned by `Handle` from here.
    unsafe {
        CreateFileW(
            PCWSTR(device.as_ptr()),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map(Handle)
    .map_err(|error| os("CreateFileW", &error))
}
