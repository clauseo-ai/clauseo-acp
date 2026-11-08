//! Windows: whether a drive letter is a network drive, by `GetDriveTypeW` on its root alone.
#![expect(unsafe_code, reason = "FFI to GetDriveTypeW")]

use windows::Win32::Storage::FileSystem::GetDriveTypeW;
use windows::core::HSTRING;

/// `GetDriveTypeW`'s answer for a network drive (`winbase.h`). Declared here so the crate's
/// `windows` line needs no `Win32_System_WindowsProgramming` feature for one constant.
const DRIVE_REMOTE: u32 = 4;

/// Whether drive `letter` is a network drive (a mapped share). Asks about `X:\` only, never a
/// path on the drive, so nothing past its root is opened.
pub(super) fn remote_drive(letter: char) -> bool {
    let root = HSTRING::from(format!("{letter}:\\"));
    // SAFETY: `root` is a NUL-terminated UTF-16 string that outlives the call, which keeps no
    // pointer to it.
    unsafe { GetDriveTypeW(&root) == DRIVE_REMOTE }
}
