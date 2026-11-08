//! Windows: whether a folder's volume is a network drive, by `GetVolumePathNameW` and
//! `GetDriveTypeW`, for [`super::place`]. The folder goes through [`long_path`], since these
//! calls do not get std's own verbatim prefix: a folder past 260 UTF-16 units on a mapped drive
//! is found remote too. Its volume root comes back verbatim and short, and goes to
//! `GetDriveTypeW` plain ([`plain`]).
#![expect(unsafe_code, reason = "FFI to GetVolumePathNameW and GetDriveTypeW")]

use std::path::Path;

use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};
use windows::core::HSTRING;

use crate::long_path::{long_path, plain};

/// `GetDriveTypeW`'s answer for a network drive (`winbase.h`). Declared here so the crate's
/// one `windows` line needs no `Win32_System_WindowsProgramming` feature for one constant.
const DRIVE_REMOTE: u32 = 4;

/// Whether the volume holding `folder` is a network drive (a mapped drive letter or a share).
/// A failed call counts as not remote; the caller still treats a UNC path as network.
pub(super) fn is_remote(folder: &Path) -> bool {
    drive_type(folder).is_some_and(|kind| kind == DRIVE_REMOTE)
}

/// `GetDriveTypeW` of the volume holding `folder`; `None` when its volume path cannot be read
/// (logged, never the path).
pub(super) fn drive_type(folder: &Path) -> Option<u32> {
    let wide = HSTRING::from(long_path(folder).as_path());
    // The volume path is never longer than the path it is taken from, plus a `\` and a NUL.
    let mut root = vec![0_u16; wide.len().saturating_add(2).max(261)];
    // SAFETY: `wide` is a NUL-terminated UTF-16 string and `root` a buffer whose length the
    // binding passes; both outlive the call, which keeps no pointer to either.
    if let Err(err) = unsafe { GetVolumePathNameW(&wide, &mut root) } {
        tracing::warn!(error = %err, "GetVolumePathNameW failed");
        return None;
    }
    let end = root
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(root.len());
    let root = HSTRING::from_wide(&plain(root.get(..end).unwrap_or_default()));
    // SAFETY: `root` is a NUL-terminated UTF-16 string that outlives the call, which keeps no
    // pointer to it.
    Some(unsafe { GetDriveTypeW(&root) })
}

#[cfg(test)]
mod tests {
    use std::os::windows::ffi::OsStrExt as _;

    use super::{DRIVE_REMOTE, drive_type, is_remote};
    use crate::test_dir::TestDir;

    /// `GetDriveTypeW`'s answer for a fixed disk (`winbase.h`).
    const DRIVE_FIXED: u32 = 3;

    /// A local folder past 300 UTF-16 units, Devanagari included, has its volume read: before
    /// [`crate::long_path`] the call failed there and the folder counted as local only by default.
    #[test]
    fn a_long_local_folder_s_volume_is_read_as_fixed() {
        let dir = TestDir::new("place-long-local");
        let mut folder = dir.path().join("मामले-अनुबंध");
        let mut n = 0;
        while folder.as_os_str().encode_wide().count() <= 300 {
            n += 1;
            folder.push(format!("{n:02}-folder-of-a-deeply-nested-client"));
        }
        crate::write::create_dir_all(&folder).unwrap();
        assert_eq!(drive_type(&folder), Some(DRIVE_FIXED));
        assert_eq!(drive_type(dir.path()), Some(DRIVE_FIXED));
        assert!(!is_remote(&folder));
    }

    /// By hand, on a Windows machine with a mapped network drive: `CLAUSEO_TEST_MAPPED_DIR`
    /// names a folder on it (`net use Z: \\localhost\c$`, then `Z:\clauseo-place`), and a
    /// folder past 300 UTF-16 units under it is found remote, as the short one is.
    #[test]
    #[ignore = "needs a mapped network drive named by CLAUSEO_TEST_MAPPED_DIR"]
    fn a_long_folder_on_a_mapped_drive_is_remote() {
        let base = std::path::PathBuf::from(
            std::env::var_os("CLAUSEO_TEST_MAPPED_DIR").expect("CLAUSEO_TEST_MAPPED_DIR"),
        );
        assert_eq!(drive_type(&base), Some(DRIVE_REMOTE));
        let mut folder = base.join("मामले-अनुबंध");
        let mut n = 0;
        while folder.as_os_str().encode_wide().count() <= 300 {
            n += 1;
            folder.push(format!("{n:02}-folder-of-a-deeply-nested-client"));
        }
        crate::write::create_dir_all(&folder).unwrap();
        let remote = is_remote(&folder);
        crate::write::remove_dir_all(&base.join("मामले-अनुबंध")).unwrap();
        assert!(remote);
    }
}
