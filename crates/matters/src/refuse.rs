//! Refused folders: an import sanity check, not a security boundary. A folder that is a drive
//! root, an entry of the host's refused list (the home folder, system folders) or an ancestor of
//! one cannot be a matter; a folder inside a listed one can. A folder that is, holds or sits
//! inside the matters root or a refused tree (the host's data folder) cannot be a matter, whether
//! it is listed or not: the history would version itself, or an agent's home would be a matter.

use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};

use unicode_normalization::UnicodeNormalization;

use crate::matter::json;
use crate::{Error, MissingReason, Refusal, Settings};

/// Refuses `folder` if, once canonicalised, it is a drive root, or equals or contains an entry of
/// `refused` (each canonicalised too, or taken as written when that fails). Paths are compared
/// by components ([`spellings`]): composed (NFC) everywhere, and ASCII- and Unicode-case-
/// insensitively on macOS and Windows.
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

/// The place check alone, before anything is read inside `folder`: refuses a drive root and an
/// entry of `refused` or an ancestor of one, as [`Matter::import`](crate::Matter::import) does
/// first (`Error::FolderRefused` with `Refusal::DriveRoot` or `Refusal::Listed`;
/// `Error::FolderMissing` for a folder that does not exist). For the host, which checks its
/// registry (a folder holding or inside another matter's) after this and before the import, so
/// a refused place is named for what it is even when it holds a matter.
pub fn check_place(folder: &Path, refused: &[PathBuf]) -> Result<(), Error> {
    check_folder(folder, refused)
}

/// Refuses `folder` if it already holds a `.clauseo/matter.json` (it belongs to a matter;
/// importing it again would take it from that matter), or for its place as
/// [`check_outside_root`] does. Called after [`check_folder`].
pub(crate) fn check_new_matter(folder: &Path, settings: &Settings) -> Result<(), Error> {
    let refuse = |reason| Error::FolderRefused {
        folder: folder.to_path_buf(),
        reason,
    };
    match json::read(folder)? {
        json::Found::NoMarker => {}
        json::Found::NoFolder => {
            return Err(Error::FolderMissing {
                folder: folder.to_path_buf(),
                reason: MissingReason::NoFolder,
            });
        }
        json::Found::Matter(id) => return Err(refuse(Refusal::AlreadyMatter(id))),
        json::Found::Unreadable => return Err(refuse(Refusal::UnreadableMatterJson)),
    }
    check_outside_root(folder, settings)
}

/// Refuses `folder` if it is, holds or sits inside the matters root (`Refusal::MattersRoot`: the
/// history would version itself), or if it is, holds or sits inside a `refused_trees` entry
/// (`Refusal::Listed(entry)`, the entry as the host passed it). Import checks it after the marker;
/// locate and relink, which need a marker, check it alone.
pub(crate) fn check_outside_root(folder: &Path, settings: &Settings) -> Result<(), Error> {
    let refuse = |reason| Error::FolderRefused {
        folder: folder.to_path_buf(),
        reason,
    };
    dunce::canonicalize(folder).map_err(|err| Error::io("canonicalize", folder, err))?;
    if same_or_nested(folder, &settings.matters_root) {
        return Err(refuse(Refusal::MattersRoot));
    }
    for tree in &settings.refused_trees {
        if same_or_nested(folder, tree) {
            return Err(refuse(Refusal::Listed(tree.clone())));
        }
    }
    Ok(())
}

/// Whether `a` and `b` are the same folder or one holds the other, compared as the refused-folder
/// check compares paths: each canonicalised (taken as written when that fails, so a folder that
/// has moved away still compares), by components, composed (NFC) everywhere, case-insensitively
/// on macOS and Windows, and on
/// macOS with and without `/System/Volumes/Data`. For the host's registry check before
/// [`Matter::locate`](crate::Matter::locate) and [`Matter::relink`](crate::Matter::relink): a
/// folder that is, contains or sits inside another registered matter's folder.
pub fn same_or_nested(a: &Path, b: &Path) -> bool {
    let keys =
        |path: &Path| spellings(&dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()));
    nested(&keys(a), &keys(b))
}

/// Whether a key of `a` starts with a key of `b`, or the other way round.
fn nested(a: &[Vec<OsString>], b: &[Vec<OsString>]) -> bool {
    a.iter().any(|a_key| {
        b.iter()
            .any(|b_key| a_key.starts_with(b_key) || b_key.starts_with(a_key))
    })
}

/// Whether `a` and `b` name the same folder once canonicalised, compared as this OS's default
/// file system compares names ([`Fold::FileSystem`]: case-insensitively on macOS and Windows,
/// and on macOS also across Unicode forms; firmlink spellings on macOS). Not the safety fold:
/// [`Matter::relink`](crate::Matter::relink) writes into the folder this names, so a folder
/// whose name only another Unicode form matches is another folder on ext4 and NTFS. A path that
/// does not canonicalise (it no longer exists) is the same as nothing.
pub(crate) fn same_folder(a: &Path, b: &Path) -> bool {
    let (Ok(a), Ok(b)) = (dunce::canonicalize(a), dunce::canonicalize(b)) else {
        return false;
    };
    let (a, b) = (keys_by(&a, Fold::FileSystem), keys_by(&b, Fold::FileSystem));
    a.iter().any(|key| b.contains(key))
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

/// How names are folded before two paths are compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fold {
    /// The safety matches, where a match refuses (refused folders, a folder holding or inside
    /// another matter's, a path checked for an owner file): composed (NFC) everywhere, so
    /// either Unicode form is caught, and lower case on macOS and Windows (decision 0007,
    /// Unicode names).
    Safety,
    /// As this OS's default file system finds names: lower case and NFC on macOS (APFS), lower
    /// case on Windows (NTFS), byte for byte elsewhere. For a match that allows something (the
    /// folder relink writes into, a path counted inside the matter): ext4 and NTFS tell the
    /// Unicode forms apart, so folding there would cover a sibling spelled in the other form.
    FileSystem,
}

/// The comparison keys of `path` for the safety matches ([`Fold::Safety`]): its components,
/// composed (NFC) and case-folded on macOS and Windows, with the Windows verbatim prefix forms
/// made plain. On macOS a key starting with `/System/Volumes/Data` comes twice: as it is and
/// without those three parts.
pub(crate) fn spellings(path: &Path) -> Vec<Vec<OsString>> {
    keys_by(path, Fold::Safety)
}

/// [`spellings`] folded by `fold`.
pub(crate) fn keys_by(path: &Path, fold: Fold) -> Vec<Vec<OsString>> {
    let key: Vec<OsString> = path
        .components()
        .map(|component| component_key(component, fold))
        .collect();
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
fn component_key(component: Component<'_>, how: Fold) -> OsString {
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
                fold(text, how)
            }
            Prefix::Verbatim(_) | Prefix::DeviceNS(_) => fold(prefix.as_os_str().to_owned(), how),
        },
        Component::RootDir => OsString::from("/"),
        Component::CurDir | Component::ParentDir | Component::Normal(_) => {
            fold(component.as_os_str().to_owned(), how)
        }
    }
}

/// `name` folded as `how` says, when it is UTF-8: lower case on the case-insensitive file
/// systems (macOS, Windows), composed (NFC) on macOS and, for [`Fold::Safety`], everywhere (ext4
/// and NTFS tell the forms apart, and refusing there is the safe side). A name that is not
/// UTF-8 is as it is.
fn fold(name: OsString, how: Fold) -> OsString {
    let Some(text) = name.to_str() else {
        return name;
    };
    let lower = cfg!(any(target_os = "macos", windows));
    let nfc = !text.is_ascii() && (how == Fold::Safety || cfg!(target_os = "macos"));
    match (lower, nfc) {
        (true, true) => OsString::from(text.to_lowercase().nfc().collect::<String>()),
        (true, false) => OsString::from(text.to_lowercase()),
        (false, true) => OsString::from(text.nfc().collect::<String>()),
        (false, false) => name,
    }
}

#[cfg(test)]
mod tests;
