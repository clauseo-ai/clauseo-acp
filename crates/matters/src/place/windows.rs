//! Windows: whether a folder's volume is a network drive, by `GetVolumePathNameW` and
//! `GetDriveTypeW`, for [`super::place`].
#![expect(unsafe_code, reason = "FFI to GetVolumePathNameW and GetDriveTypeW")]

use std::path::Path;

use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};
use windows::core::HSTRING;

/// `GetDriveTypeW`'s answer for a network drive (`winbase.h`). Declared here so the crate's
/// one `windows` line needs no `Win32_System_WindowsProgramming` feature for one constant.
const DRIVE_REMOTE: u32 = 4;

/// Whether the volume holding `folder` is a network drive (a mapped drive letter or a share).
/// A failed call counts as not remote; the caller still treats a UNC path as network.
pub(super) fn is_remote(folder: &Path) -> bool {
    let wide = HSTRING::from(folder);
    // The volume path is never longer than the path it is taken from, plus a `\` and a NUL.
    let mut root = vec![0_u16; wide.len().saturating_add(2).max(261)];
    // SAFETY: `wide` is a NUL-terminated UTF-16 string and `root` a buffer whose length the
    // binding passes; both outlive the call, which keeps no pointer to either.
    if let Err(err) = unsafe { GetVolumePathNameW(&wide, &mut root) } {
        tracing::warn!(error = %err, "GetVolumePathNameW failed");
        return false;
    }
    let end = root
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(root.len());
    let root = HSTRING::from_wide(root.get(..end).unwrap_or_default());
    // SAFETY: `root` is a NUL-terminated UTF-16 string that outlives the call, which keeps no
    // pointer to it.
    unsafe { GetDriveTypeW(&root) == DRIVE_REMOTE }
}
