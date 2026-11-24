//! Defender's Controlled folder access on Windows: its preference read through PowerShell's
//! `Get-MpPreference` (Defender has no documented API for it), and the folders it protects by
//! default, from the known-folder lookup the folder chooser already makes.

use std::ffi::OsStr;
use std::path::PathBuf;

use clauseo_settings::{KnownFolder, PlatformDirs as _, SystemDirs};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Favorites, FOLDERID_Music, FOLDERID_Pictures,
    FOLDERID_Videos,
};

use super::system::{PROBE_TIMEOUT, run};

/// The preference's three fields, as JSON.
const QUERY: &str = "Get-MpPreference | Select-Object EnableControlledFolderAccess,\
                     ControlledFolderAccessProtectedFolders,\
                     ControlledFolderAccessAllowedApplications | ConvertTo-Json";

/// `Get-MpPreference`'s JSON, from Windows' own PowerShell (by its full path, no profile, no
/// prompt, no console window), or `None` when it does not start, fails or outlasts
/// [`PROBE_TIMEOUT`].
pub(super) fn preference_text() -> Option<String> {
    let powershell = SystemDirs
        .known_folder(KnownFolder::Windows)?
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let args = ["-NoProfile", "-NonInteractive", "-Command", QUERY].map(OsStr::new);
    match run(&powershell, &args, Some(PROBE_TIMEOUT)) {
        Ok(Some((Some(0), out))) => Some(String::from_utf8_lossy(&out).into_owned()),
        Ok(Some((code, _))) => {
            tracing::info!(?code, "permissions: Get-MpPreference failed");
            None
        }
        Ok(None) => {
            tracing::info!("permissions: Get-MpPreference did not answer in time");
            None
        }
        Err(error) => {
            tracing::info!(%error, "permissions: Get-MpPreference did not start");
            None
        }
    }
}

/// The folders Controlled folder access protects whatever its list says: the user's Documents,
/// Desktop, Pictures, Videos, Music and Favorites, those Windows gives.
pub(super) fn protected_defaults() -> Vec<PathBuf> {
    [
        &FOLDERID_Documents,
        &FOLDERID_Desktop,
        &FOLDERID_Pictures,
        &FOLDERID_Videos,
        &FOLDERID_Music,
        &FOLDERID_Favorites,
    ]
    .into_iter()
    .filter_map(crate::chooser::known_folder)
    .map(PathBuf::from)
    .collect()
}
