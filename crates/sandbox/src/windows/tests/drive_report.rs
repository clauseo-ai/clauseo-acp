//! One-off for PR #822: what the temp folder's drive and `C:` report to each call a drive
//! classifier could read. Printed, never asserted; the ci profile shows this test's output when it
//! passes (`.config/nextest.toml`). Remove both once the runner's numbers are in decision 0049.
//! Its FFI falls under `windows/tests.rs`'s `#![expect(unsafe_code)]`, so it adds no
//! allowlist line.

use windows::Wdk::Storage::FileSystem::{FileFsDeviceInformation, NtQueryVolumeInformationFile};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDriveTypeW,
    OPEN_EXISTING, QueryDosDeviceW,
};
use windows::Win32::System::IO::{DeviceIoControl, IO_STATUS_BLOCK};
use windows::Win32::System::Ioctl::{
    IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery, STORAGE_DEVICE_DESCRIPTOR,
    STORAGE_PROPERTY_QUERY, StorageDeviceProperty,
};
use windows::core::PCWSTR;

use crate::links::network::{Mounts, classify};

#[test]
fn the_temp_drive_reports() {
    let temp = std::env::temp_dir();
    let text = temp.to_string_lossy().into_owned();
    let mut letters = vec![b'C'];
    match text.as_bytes() {
        [letter, b':', ..] if letter.is_ascii_alphabetic() => {
            letters.insert(0, letter.to_ascii_uppercase());
        }
        _ => eprintln!("drive report: the temp folder {text} has no drive letter"),
    }
    letters.dedup();
    let mounts = Mounts::current();
    for letter in letters {
        let drive = format!("{}:", char::from(letter));
        eprintln!("drive report for {drive} (temp folder {text}):");
        eprintln!("  QueryDosDeviceW: {}", dos_devices(&drive));
        eprintln!("  GetDriveTypeW: {}", drive_type(&drive));
        let root = std::path::PathBuf::from(format!(r"{drive}\"));
        eprintln!("  classify: {:?}", classify(&root, &mounts));
        match open_volume(&drive) {
            Ok(volume) => {
                eprintln!("  FileFsDeviceInformation: {}", device_information(&volume));
                eprintln!("  IOCTL_STORAGE_QUERY_PROPERTY bus: {}", bus_type(&volume));
            }
            Err(error) => eprintln!("  opening \\\\.\\{drive} with no access failed: {error}"),
        }
    }
}

/// `text` as NUL-terminated UTF-16.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Every target of the DOS device `drive` (`D:`), or the error.
fn dos_devices(drive: &str) -> String {
    let name = wide(drive);
    let mut target = vec![0_u16; 4096];
    // SAFETY: `name` is NUL-terminated and outlives the call; `target` is writable and its length
    // is passed by the binding.
    let written = unsafe { QueryDosDeviceW(PCWSTR(name.as_ptr()), Some(&mut target)) };
    if written == 0 {
        return format!("failed: {}", windows::core::Error::from_thread());
    }
    let written = usize::try_from(written).unwrap_or(0);
    target
        .get(..written)
        .unwrap_or_default()
        .split(|unit| *unit == 0)
        .filter(|part| !part.is_empty())
        .map(String::from_utf16_lossy)
        .collect::<Vec<_>>()
        .join(" | ")
}

/// `GetDriveTypeW` of `drive`'s root, as its number and name.
fn drive_type(drive: &str) -> String {
    let root = wide(&format!(r"{drive}\"));
    // SAFETY: `root` is NUL-terminated and outlives the call.
    let kind = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
    let name = match kind {
        0 => "DRIVE_UNKNOWN",
        1 => "DRIVE_NO_ROOT_DIR",
        2 => "DRIVE_REMOVABLE",
        3 => "DRIVE_FIXED",
        4 => "DRIVE_REMOTE",
        5 => "DRIVE_CDROM",
        6 => "DRIVE_RAMDISK",
        _ => "?",
    };
    format!("{kind} {name}")
}

/// A volume handle, closed on drop.
struct Volume(HANDLE);

impl Drop for Volume {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful CreateFileW and is closed only here.
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            eprintln!("  CloseHandle failed: {error}");
        }
    }
}

/// `\\.\X:` opened with no access, which reads the device's metadata without mounting or
/// touching its file system.
fn open_volume(drive: &str) -> windows::core::Result<Volume> {
    let path = wide(&format!(r"\\.\{drive}"));
    // SAFETY: `path` is NUL-terminated and outlives the call; no security attributes or template.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }?;
    Ok(Volume(handle))
}

/// `FILE_FS_DEVICE_INFORMATION`, declared here so the test needs no other `windows` feature.
#[repr(C)]
#[derive(Default)]
struct DeviceInformation {
    device_type: u32,
    characteristics: u32,
}

/// The volume device's type and characteristics, `FILE_REMOTE_DEVICE` (0x10) named.
fn device_information(volume: &Volume) -> String {
    let mut info = DeviceInformation::default();
    let mut status = IO_STATUS_BLOCK::default();
    let Ok(size) = u32::try_from(size_of::<DeviceInformation>()) else {
        return "size".to_owned();
    };
    // SAFETY: the handle is open; `status` and `info` are live and writable, `info` of `size`
    // bytes.
    let answer = unsafe {
        NtQueryVolumeInformationFile(
            volume.0,
            &raw mut status,
            (&raw mut info).cast(),
            size,
            FileFsDeviceInformation,
        )
    };
    if answer.is_err() {
        return format!("failed: NTSTATUS {:#010x}", answer.0);
    }
    format!(
        "device type {:#x}, characteristics {:#x}, FILE_REMOTE_DEVICE {}",
        info.device_type,
        info.characteristics,
        info.characteristics & 0x10 != 0
    )
}

/// The storage bus of the volume's disk (`STORAGE_BUS_TYPE`: 1 SCSI, 3 ATA, 7 USB, 8 RAID,
/// 9 iSCSI, 10 SAS, 11 SATA, 14 Virtual, 15 `FileBackedVirtual`, 17 `NVMe`).
fn bus_type(volume: &Volume) -> String {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        ..STORAGE_PROPERTY_QUERY::default()
    };
    let mut out = vec![0_u64; 128];
    let (Ok(size), Ok(query_size)) = (
        u32::try_from(out.len() * 8),
        u32::try_from(size_of::<STORAGE_PROPERTY_QUERY>()),
    ) else {
        return "size".to_owned();
    };
    let mut returned = 0_u32;
    // SAFETY: the handle is open; `query` is live and of the size given; `out` is writable for
    // `size` bytes and `returned` is a live out-pointer.
    let asked = unsafe {
        DeviceIoControl(
            volume.0,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some((&raw const query).cast()),
            query_size,
            Some(out.as_mut_ptr().cast()),
            size,
            Some(&raw mut returned),
            None,
        )
    };
    if let Err(error) = asked {
        return format!("failed: {error}");
    }
    let bytes: Vec<u8> = out.iter().flat_map(|word| word.to_ne_bytes()).collect();
    let at = std::mem::offset_of!(STORAGE_DEVICE_DESCRIPTOR, BusType);
    bytes
        .get(at..at + 4)
        .and_then(|four| four.try_into().ok())
        .map_or_else(
            || "short answer".to_owned(),
            |four| i32::from_le_bytes(four).to_string(),
        )
}
