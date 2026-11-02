//! Where a folder lives: on this computer, on a network drive, or in a folder a cloud service
//! syncs. The rules are plain functions of what the OS says ([`mac_place`], [`windows_place`]),
//! so every OS's rules are tested on every OS; [`place`] asks this OS and applies its rule.
//!
//! Detected: Apple's `CloudStorage` (`~/Library/CloudStorage/<Provider>-*`), iCloud Drive
//! (`~/Library/Mobile Documents`) and `OneDrive` on Windows (`%OneDrive%`,
//! `%OneDriveCommercial%`, `%OneDriveConsumer%`). Other services are not looked for. On Linux
//! every folder is [`Place::Local`].

use std::path::Path;

#[cfg(any(test, windows))]
use crate::refuse;

#[cfg(windows)]
mod windows;

#[cfg(test)]
mod tests;

/// Where a folder lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// On a disk of this computer, and no cloud service we know syncs it.
    Local,
    /// On a network drive or share.
    Network,
    /// In a folder a cloud service syncs.
    Synced {
        /// The service, from the synced folder's name: `OneDrive`, `GoogleDrive`,
        /// `iCloud Drive`.
        service: String,
    },
}

#[cfg(any(test, target_os = "macos"))]
/// The service iCloud Drive's folder (`~/Library/Mobile Documents`) names.
const ICLOUD_DRIVE: &str = "iCloud Drive";

/// The Windows variables that hold a `OneDrive` folder, in the order they are tried.
#[cfg(windows)]
const ONEDRIVE_VARS: [&str; 3] = ["OneDrive", "OneDriveCommercial", "OneDriveConsumer"];

/// Where `folder` lives, by this OS's rule. Asks the OS for the folder's mount (macOS
/// `statfs`) or drive type (Windows `GetDriveTypeW`) and reads `HOME` (macOS) or the `OneDrive`
/// variables (Windows). A failed call counts as local. Reads no file.
pub fn place(folder: &Path) -> Place {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        mac_place(folder, home.as_deref(), mac_local_mount(folder))
    }
    #[cfg(windows)]
    {
        let roots: Vec<String> = ONEDRIVE_VARS
            .iter()
            .filter_map(std::env::var_os)
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        let text = folder.to_string_lossy();
        windows_place(&text, &roots, windows::is_remote(folder))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = folder;
        Place::Local
    }
}

/// macOS: whether the file system holding `folder` is local (`MNT_LOCAL` in its `statfs`
/// flags); `None` when `statfs` fails.
#[cfg(target_os = "macos")]
fn mac_local_mount(folder: &Path) -> Option<bool> {
    use nix::mount::MntFlags;
    match nix::sys::statfs::statfs(folder) {
        Ok(stat) => Some(stat.flags().contains(MntFlags::MNT_LOCAL)),
        Err(err) => {
            tracing::warn!(path = %folder.display(), error = %err, "statfs failed");
            None
        }
    }
}

#[cfg(any(test, target_os = "macos"))]
/// The macOS rule. Synced: `folder` is under `<home>/Library/CloudStorage/<name>` (service
/// from `<name>`) or `<home>/Library/Mobile Documents` (iCloud Drive). Else network when
/// `local_mount` is `Some(false)` (no `MNT_LOCAL`). Else local.
pub(crate) fn mac_place(folder: &Path, home: Option<&Path>, local_mount: Option<bool>) -> Place {
    if let Some(library) = home.map(|home| home.join("Library")) {
        if let Ok(rest) = folder.strip_prefix(library.join("CloudStorage"))
            && let Some(name) = rest.components().next()
        {
            let name = name.as_os_str().to_string_lossy();
            return Place::Synced {
                service: service_name(&name),
            };
        }
        if folder.starts_with(library.join("Mobile Documents")) {
            return Place::Synced {
                service: ICLOUD_DRIVE.to_owned(),
            };
        }
    }
    if local_mount == Some(false) {
        Place::Network
    } else {
        Place::Local
    }
}

#[cfg(any(test, windows))]
/// The Windows rule, on paths as text so it runs on every OS. Synced: `folder` is, or is
/// under, one of `onedrive_roots` (the `OneDrive` variables' values), compared without case;
/// the service is from that root's name. Else network when `remote` (`GetDriveTypeW` says
/// `DRIVE_REMOTE` for the folder's volume) or `folder` is a UNC path. Else local.
pub(crate) fn windows_place(folder: &str, onedrive_roots: &[String], remote: bool) -> Place {
    let folder_key = windows_key(folder);
    for root in onedrive_roots {
        let root_key = windows_key(root);
        if root_key.is_empty() {
            continue;
        }
        let inside = folder_key
            .strip_prefix(root_key.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('\\'));
        if inside {
            let trimmed = root.trim_end_matches(['\\', '/']);
            let name = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
            return Place::Synced {
                service: service_name(name),
            };
        }
    }
    if remote || refuse::is_unc_text(folder) {
        Place::Network
    } else {
        Place::Local
    }
}

#[cfg(any(test, windows))]
/// A Windows path made comparable: `/` as `\`, no `\\?\` before a drive letter, no trailing
/// `\`, lower case.
fn windows_key(path: &str) -> String {
    let path = path.replace('/', "\\");
    let path = match path.strip_prefix(r"\\?\") {
        Some(rest) if !refuse::is_unc_text(&path) => rest.to_owned(),
        _ => path,
    };
    path.trim_end_matches('\\').to_lowercase()
}

#[cfg(any(test, target_os = "macos", windows))]
/// The service a synced folder's `name` gives: what comes before its first `-`, trimmed
/// (`OneDrive-Contoso` and `OneDrive - Contoso` are `OneDrive`), or the whole name.
pub(crate) fn service_name(name: &str) -> String {
    let service = name.split('-').next().unwrap_or(name).trim();
    if service.is_empty() {
        name.trim().to_owned()
    } else {
        service.to_owned()
    }
}
