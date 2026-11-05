//! Tests' directory junctions: [`junction`] makes one without admin rights, as `mklink /J` does,
//! by setting a mount-point reparse point on a new empty folder. For tests only: this crate's,
//! and with `test-support` the host's.
#![expect(
    unsafe_code,
    reason = "FFI to open a test folder and set its mount-point reparse point"
)]
#![expect(
    clippy::disallowed_methods,
    reason = "the test junction's empty folder is made, and removed on failure, here alone"
)]

use std::io;
use std::iter::once;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_NONE,
    OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::core::PCWSTR;

/// `FSCTL_SET_REPARSE_POINT`.
const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
/// `IO_REPARSE_TAG_MOUNT_POINT`: a directory junction.
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

/// Makes `link` (which must not exist; `target` absolute, an existing folder) a directory
/// junction to `target`.
pub fn junction(link: &Path, target: &Path) -> io::Result<()> {
    std::fs::create_dir(link)?;
    let made = set_mount_point(link, target);
    if made.is_err() {
        drop(std::fs::remove_dir(link));
    }
    made
}

fn set_mount_point(link: &Path, target: &Path) -> io::Result<()> {
    let target = std::path::absolute(target)?;
    let print: Vec<u16> = target.as_os_str().encode_wide().collect();
    let mut substitute: Vec<u16> = r"\??\".encode_utf16().collect();
    substitute.extend_from_slice(&print);
    let unit = |n: usize| u16::try_from(n * 2).map_err(io::Error::other);
    let (sub_len, print_len) = (unit(substitute.len())?, unit(print.len())?);
    // The mount-point buffer: tag, data length, reserved, then the substitute name's offset and
    // length, the print name's offset and length, and both names, each NUL-terminated.
    let mut names = substitute.clone();
    names.push(0);
    names.extend_from_slice(&print);
    names.push(0);
    let data_len = unit(4 + names.len())?;
    let mut buffer: Vec<u8> = Vec::new();
    buffer.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    buffer.extend_from_slice(&data_len.to_le_bytes());
    buffer.extend_from_slice(&0u16.to_le_bytes());
    buffer.extend_from_slice(&0u16.to_le_bytes());
    buffer.extend_from_slice(&sub_len.to_le_bytes());
    buffer.extend_from_slice(&(sub_len + 2).to_le_bytes());
    buffer.extend_from_slice(&print_len.to_le_bytes());
    for unit in names {
        buffer.extend_from_slice(&unit.to_le_bytes());
    }
    let wide: Vec<u16> = link.as_os_str().encode_wide().chain(once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call; no security attributes or template.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_WRITE.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }?;
    let size = u32::try_from(buffer.len()).map_err(io::Error::other)?;
    // SAFETY: the handle is open; `buffer` is live and `size` bytes long; no output buffer.
    let set = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_SET_REPARSE_POINT,
            Some(buffer.as_ptr().cast()),
            size,
            None,
            0,
            None,
            None,
        )
    };
    // SAFETY: the handle came from CreateFileW above and is closed once, here.
    drop(unsafe { CloseHandle(handle) });
    set.map_err(io::Error::other)
}
