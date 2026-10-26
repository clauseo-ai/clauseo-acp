//! Refused folders: an import sanity check, not a security boundary. A folder that is a drive
//! root, an entry of the host's refused list (the home folder, system folders) or an ancestor of
//! one cannot be a matter; a folder inside a listed one can.

use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};

use crate::{Error, MissingReason, Refusal};

/// Refuses `folder` if, once canonicalised, it is a drive root, or equals or contains an entry of
/// `refused` (each canonicalised too, or taken as written when that fails). Paths are compared
/// by components: ASCII- and Unicode-case-insensitively on macOS and Windows, exactly elsewhere.
/// On macOS a path under `/System/Volumes/Data` is also compared without that prefix.
pub(crate) fn check_folder(folder: &Path, refused: &[PathBuf]) -> Result<(), Error> {
    let canonical = dunce::canonicalize(folder).map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            Error::FolderMissing {
                folder: folder.to_path_buf(),
                reason: MissingReason::NoFolder,
            }
        } else {
            Error::io("canonicalize", folder, err)
        }
    })?;
    let refuse = |reason| Error::FolderRefused {
        folder: folder.to_path_buf(),
        reason,
    };
    if is_drive_root(&canonical) {
        return Err(refuse(Refusal::DriveRoot));
    }
    #[cfg(target_os = "linux")]
    if is_mount_point(&canonical)? {
        return Err(refuse(Refusal::DriveRoot));
    }
    let candidate = spellings(&canonical);
    for entry in refused {
        let resolved = dunce::canonicalize(entry).unwrap_or_else(|_| entry.clone());
        let listed = spellings(&resolved);
        let contains = candidate.iter().any(|outer| {
            listed
                .iter()
                .any(|inner| inner.starts_with(outer.as_slice()))
        });
        if contains {
            return Err(refuse(Refusal::Listed(entry.clone())));
        }
    }
    Ok(())
}

/// Whether `folder` is on a network share: `\\server\share...` or `\\?\UNC\...`.
pub(crate) fn is_unc(folder: &Path) -> bool {
    is_unc_text(&folder.to_string_lossy())
}

/// Whether `text` is a UNC path: `\\server\share...` or `\\?\UNC\...`, but not `\\?\X:...`
/// or a `\\.\` device path. A Unix path never starts with `\\`, so this holds on every OS.
pub(crate) fn is_unc_text(text: &str) -> bool {
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return rest
            .get(..4)
            .is_some_and(|head| head.eq_ignore_ascii_case(r"UNC\"));
    }
    if text.starts_with(r"\\.\") {
        return false;
    }
    text.strip_prefix(r"\\")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|first| first != '\\' && first != '/')
}

/// Whether `text` is a Windows drive root: a drive letter, `:` and an optional separator, with
/// nothing after it, also in the `\\?\X:\` form.
pub(crate) fn is_windows_drive_root_text(text: &str) -> bool {
    let rest = text.strip_prefix(r"\\?\").unwrap_or(text);
    match rest.as_bytes() {
        [letter, b':'] | [letter, b':', b'\\' | b'/'] => letter.is_ascii_alphabetic(),
        _ => false,
    }
}

/// Whether the canonical `folder` is a drive root by its path: `X:\` on every OS (an absolute
/// Unix path never matches it), `/`, and on macOS `/Volumes/<name>`.
fn is_drive_root(folder: &Path) -> bool {
    if is_windows_drive_root_text(&folder.to_string_lossy()) {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        spellings(folder).iter().any(|key| is_macos_volume_key(key))
    }
    #[cfg(not(target_os = "macos"))]
    {
        cfg!(unix) && folder.parent().is_none()
    }
}

/// Linux: whether `folder` is a mount point, its device differing from its parent's.
#[cfg(target_os = "linux")]
fn is_mount_point(folder: &Path) -> Result<bool, Error> {
    use std::os::unix::fs::MetadataExt;
    let Some(parent) = folder.parent() else {
        return Ok(true);
    };
    let dev = |path: &Path| {
        std::fs::metadata(path)
            .map(|metadata| metadata.dev())
            .map_err(|err| Error::io("stat", path, err))
    };
    Ok(dev(folder)? != dev(parent)?)
}

/// Whether a macOS comparison key is `/` or `/Volumes/<name>`.
#[cfg(target_os = "macos")]
fn is_macos_volume_key(key: &[OsString]) -> bool {
    match key {
        [_root] => true,
        [_root, volumes, _name] => volumes == "volumes",
        _ => false,
    }
}

/// The comparison keys of `path`: its components, case-folded on macOS and Windows, with the
/// Windows verbatim prefix forms made plain. On macOS a key starting with
/// `/System/Volumes/Data` comes twice: as it is and without those three parts.
fn spellings(path: &Path) -> Vec<Vec<OsString>> {
    let key: Vec<OsString> = path.components().map(component_key).collect();
    let mut keys = Vec::with_capacity(2);
    #[cfg(target_os = "macos")]
    if let [_root, system, volumes, data, ..] = key.as_slice()
        && system == "system"
        && volumes == "volumes"
        && data == "data"
    {
        let mut stripped = key.clone();
        stripped.drain(1..4);
        keys.push(stripped);
    }
    keys.push(key);
    keys
}

/// One component's comparison key.
fn component_key(component: Component<'_>) -> OsString {
    match component {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                OsString::from(format!("{}:", char::from(letter.to_ascii_lowercase())))
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                let mut text = OsString::from(r"\\");
                text.push(server);
                text.push(r"\");
                text.push(share);
                fold(text)
            }
            Prefix::Verbatim(_) | Prefix::DeviceNS(_) => fold(prefix.as_os_str().to_owned()),
        },
        Component::RootDir => OsString::from("/"),
        Component::CurDir | Component::ParentDir | Component::Normal(_) => {
            fold(component.as_os_str().to_owned())
        }
    }
}

/// `name` lowercased on the case-insensitive file systems (macOS, Windows) when it is UTF-8;
/// as it is elsewhere.
fn fold(name: OsString) -> OsString {
    if cfg!(any(target_os = "macos", windows)) {
        match name.to_str() {
            Some(text) => OsString::from(text.to_lowercase()),
            None => name,
        }
    } else {
        name
    }
}

#[cfg(test)]
mod tests;
