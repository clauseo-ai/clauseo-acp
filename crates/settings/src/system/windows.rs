//! Windows known folders, through `SHGetKnownFolderPath`.
#![expect(unsafe_code, reason = "FFI to the Windows known-folder API")]

use crate::locations::KnownFolder;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_ProgramData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86,
    FOLDERID_Public, FOLDERID_RoamingAppData, FOLDERID_Windows, KF_FLAG_DEFAULT,
    SHGetKnownFolderPath,
};
use windows::core::GUID;

/// The folder on this machine, or `None` when it has none.
pub(super) fn folder(folder: KnownFolder) -> Option<PathBuf> {
    let id = match folder {
        KnownFolder::RoamingAppData => FOLDERID_RoamingAppData,
        KnownFolder::LocalAppData => FOLDERID_LocalAppData,
        KnownFolder::Windows => FOLDERID_Windows,
        KnownFolder::ProgramFiles => FOLDERID_ProgramFiles,
        KnownFolder::ProgramFilesX86 => FOLDERID_ProgramFilesX86,
        KnownFolder::ProgramData => FOLDERID_ProgramData,
        KnownFolder::Public => FOLDERID_Public,
        // No known folder names it: it sits beside `Program Files` on ARM64 Windows.
        KnownFolder::ProgramFilesArm => {
            return known_folder(&FOLDERID_ProgramFiles)
                .and_then(|files| Some(files.parent()?.join("Program Files (Arm)")))
                .filter(|arm| arm.is_dir());
        }
    };
    known_folder(&id)
}

/// A known folder's path, or `None` when the call fails.
pub(super) fn known_folder(id: &GUID) -> Option<PathBuf> {
    // SAFETY: `id` points to a valid GUID for the whole call, and no access token is passed, so
    // the call reads the current user's folder.
    let path = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?;
    // SAFETY: on success the call returns a NUL-terminated wide string it allocated.
    let wide = unsafe { path.as_wide() };
    let folder = PathBuf::from(OsString::from_wide(wide));
    // SAFETY: the string came from SHGetKnownFolderPath, which says to free it with
    // CoTaskMemFree, and `wide`, its only borrow, is no longer used.
    unsafe { CoTaskMemFree(Some(path.as_ptr().cast())) };
    Some(folder)
}
