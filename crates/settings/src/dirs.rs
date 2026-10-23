//! The folders a lawyer may not pick as a matter: [`refused_folders`].

#[cfg(test)]
mod tests;

use crate::locations::{KnownFolder, Os, PlatformDirs};
use std::path::{Path, PathBuf};

/// Folders inside home that hold app data and keys, on every OS.
const HOME_DOT_FOLDERS: [&str; 6] = [".config", ".local", ".cache", ".ssh", ".gnupg", ".aws"];

/// macOS system folders.
const MACOS_SYSTEM: [&str; 12] = [
    "/System",
    "/Library",
    "/Applications",
    "/usr",
    "/bin",
    "/sbin",
    "/etc",
    "/private",
    "/opt",
    "/Volumes",
    "/cores",
    "/dev",
];

/// Linux system folders.
const LINUX_SYSTEM: [&str; 18] = [
    "/usr", "/bin", "/sbin", "/lib", "/lib64", "/etc", "/boot", "/proc", "/sys", "/dev", "/run",
    "/var", "/opt", "/srv", "/root", "/snap", "/mnt", "/media",
];

/// Windows known folders that are system or app data folders.
const WINDOWS_KNOWN: [KnownFolder; 8] = [
    KnownFolder::RoamingAppData,
    KnownFolder::LocalAppData,
    KnownFolder::Windows,
    KnownFolder::ProgramFiles,
    KnownFolder::ProgramFilesX86,
    KnownFolder::ProgramFilesArm,
    KnownFolder::ProgramData,
    KnownFolder::Public,
];

/// Folders at the root of the Windows system drive.
const WINDOWS_DRIVE_ROOT: [&str; 2] = ["$Recycle.Bin", "System Volume Information"];

/// The home folder, the folders inside it that hold app data and keys, and the OS's system
/// folders, as written (not canonicalised). `host` adds the data folder, and `matters` refuses
/// drive roots, each listed folder and any folder above one. This is a sanity check on import,
/// not a security boundary.
pub fn refused_folders(dirs: &dyn PlatformDirs) -> Vec<PathBuf> {
    refused_for(Os::CURRENT, dirs)
}

pub(crate) fn refused_for(os: Os, dirs: &dyn PlatformDirs) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    let home = dirs.home();
    if let Some(home) = &home {
        folders.push(home.clone());
        folders.extend(HOME_DOT_FOLDERS.iter().map(|name| home.join(name)));
    }
    match os {
        Os::Mac => {
            folders.extend(home.iter().map(|home| home.join("Library")));
            folders.extend(MACOS_SYSTEM.iter().map(PathBuf::from));
        }
        Os::Linux => folders.extend(LINUX_SYSTEM.iter().map(PathBuf::from)),
        Os::Windows => {
            folders.extend(
                WINDOWS_KNOWN
                    .iter()
                    .filter_map(|&folder| dirs.known_folder(folder)),
            );
            folders.extend(
                home.as_deref()
                    .and_then(Path::parent)
                    .map(|users| users.join("Default")),
            );
            let drive_root = dirs
                .known_folder(KnownFolder::Windows)
                .and_then(|windows| windows.ancestors().last().map(Path::to_owned));
            if let Some(root) = drive_root {
                folders.extend(WINDOWS_DRIVE_ROOT.iter().map(|name| root.join(name)));
            }
        }
    }
    folders
}
