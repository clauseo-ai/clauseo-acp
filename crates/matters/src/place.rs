//! Where a folder lives: on this computer, on a network drive, or in a folder a cloud service
//! syncs. The rules are plain functions of what the OS says ([`mac_place`], [`windows_place`]),
//! so every OS's rules are tested on every OS; [`place`] asks this OS and applies its rule.
//!
//! Detected: Apple's `CloudStorage` (`~/Library/CloudStorage/<Provider>-*`), iCloud Drive
//! (`~/Library/Mobile Documents`) and `OneDrive` on Windows (`%OneDrive%`,
//! `%OneDriveCommercial%`, `%OneDriveConsumer%`). Other services are not looked for. On Linux
//! every folder is [`Place::Local`].

use std::path::Path;

#[cfg(any(test, windows, target_os = "macos"))]
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
/// from `<name>`) or `<home>/Library/Mobile Documents` (iCloud Drive), compared by their
/// macOS spellings (`refuse::mac_spellings`: without case, and `/System/Volumes/Data/Users/..`
/// as `/Users/..`). Else network when `local_mount` is `Some(false)` (no `MNT_LOCAL`). Else
/// local.
pub(crate) fn mac_place(folder: &Path, home: Option<&Path>, local_mount: Option<bool>) -> Place {
    if let Some(home) = home {
        if let Some(rest) = mac_rest(folder, home, &["library", "cloudstorage"])
            && let Some(name) = rest.first()
        {
            return Place::Synced {
                service: service_name(name),
            };
        }
        if mac_rest(folder, home, &["library", "mobile documents"]).is_some() {
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

#[cfg(any(test, target_os = "macos"))]
/// When `folder` is, or is under, `home` joined with `tail` (lowercase parts), by any pair of
/// their macOS spellings: the parts of `folder` below that, as `folder` spells them.
fn mac_rest(folder: &Path, home: &Path, tail: &[&str]) -> Option<Vec<String>> {
    let parts: Vec<_> = folder.components().collect();
    for key in refuse::mac_spellings(folder) {
        // A key without the data volume's three parts is shorter than `parts` by three.
        let offset = parts.len() - key.len();
        for home_key in refuse::mac_spellings(home) {
            let mut prefix = home_key;
            prefix.extend(tail.iter().map(std::ffi::OsString::from));
            if key.starts_with(&prefix) {
                let below = parts.iter().skip(offset + prefix.len());
                return Some(
                    below
                        .map(|part| part.as_os_str().to_string_lossy().into_owned())
                        .collect(),
                );
            }
        }
    }
    None
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
/// A Windows path made comparable: `/` as `\`, the verbatim forms made plain (`\\?\C:\..` as
/// `C:\..`, `\\?\UNC\server\share\..` as `\\server\share\..`), no trailing `\`, lower case.
fn windows_key(path: &str) -> String {
    let path = path.replace('/', "\\");
    let plain = match path.strip_prefix(r"\\?\") {
        Some(rest) => match rest.get(..4).zip(rest.get(4..)) {
            Some((head, share)) if head.eq_ignore_ascii_case(r"UNC\") => format!(r"\\{share}"),
            _ if rest.as_bytes().get(1) == Some(&b':') => rest.to_owned(),
            _ => path.clone(),
        },
        None => path,
    };
    plain.trim_end_matches('\\').to_lowercase()
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
