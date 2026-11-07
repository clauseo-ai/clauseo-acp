//! Windows known folders, through `SHGetKnownFolderPath`.
#![expect(unsafe_code, reason = "FFI to the Windows known-folder API")]

use super::bounded;
use crate::locations::KnownFolder;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;
use std::sync::OnceLock;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Documents, FOLDERID_LocalAppData, FOLDERID_ProgramData, FOLDERID_ProgramFiles,
    FOLDERID_ProgramFilesX86, FOLDERID_Public, FOLDERID_RoamingAppData, FOLDERID_Windows,
    KF_FLAG_DEFAULT, KF_FLAG_DONT_VERIFY, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath,
};
use windows::core::GUID;

/// The folder on this machine, or `None` when it has none.
pub(super) fn folder(folder: KnownFolder) -> Option<PathBuf> {
    let id = match folder {
        // Unchecked: only the refused folders need it, by path, and the host's start builds them
        // (`refused_folders`); a firm may redirect it to a share that is down (`documents`).
        KnownFolder::RoamingAppData => {
            return known_folder_with(&FOLDERID_RoamingAppData, KF_FLAG_DONT_VERIFY);
        }
        // Unchecked: the data folder is resolved on the main thread before the first window, so it
        // must never touch the disk; Clauseo creates the data folder itself (`startup::open`).
        KnownFolder::LocalAppData => {
            return known_folder_with(&FOLDERID_LocalAppData, KF_FLAG_DONT_VERIFY);
        }
        KnownFolder::Documents => return documents(),
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

/// The Documents folder, if it is there, looked up once per process with a time limit. Windows
/// is told not to check the folder (`KF_FLAG_DONT_VERIFY`, a registry read), since its check
/// touches the network for a redirected folder and can hang on a share that is down; a timed
/// `metadata` call checks it instead ([`bounded::verified_dir_once`]): one check per process.
fn documents() -> Option<PathBuf> {
    static DOCUMENTS: OnceLock<Result<PathBuf, bounded::Unavailable>> = OnceLock::new();
    bounded::verified_dir_once(
        &DOCUMENTS,
        "Documents",
        bounded::DOCUMENTS_WAIT,
        || known_folder_with(&FOLDERID_Documents, KF_FLAG_DONT_VERIFY),
        |folder| std::fs::metadata(folder).is_ok_and(|metadata| metadata.is_dir()),
    )
}

/// A known folder's path, or `None` when the call fails.
pub(super) fn known_folder(id: &GUID) -> Option<PathBuf> {
    known_folder_with(id, KF_FLAG_DEFAULT)
}

/// [`known_folder`] with `flags`.
fn known_folder_with(id: &GUID, flags: KNOWN_FOLDER_FLAG) -> Option<PathBuf> {
    // SAFETY: `id` points to a valid GUID for the whole call, and no access token is passed, so
    // the call reads the current user's folder.
    let path = unsafe { SHGetKnownFolderPath(id, flags, None) }.ok()?;
    // SAFETY: on success the call returns a NUL-terminated wide string it allocated.
    let wide = unsafe { path.as_wide() };
    let folder = PathBuf::from(OsString::from_wide(wide));
    // SAFETY: the string came from SHGetKnownFolderPath, which says to free it with
    // CoTaskMemFree, and `wide`, its only borrow, is no longer used.
    unsafe { CoTaskMemFree(Some(path.as_ptr().cast())) };
    Some(folder)
}
