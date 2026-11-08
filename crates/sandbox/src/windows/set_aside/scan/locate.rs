//! Where a link's or a `commondir`'s target lies, told by identity, not by spelling (rule 8). The
//! target is worked out from its words, relative to the folder it was written in, and never
//! opened through a link. It is then walked one part at a time, each part opened relative to the
//! one before as itself: from the watched folder when its words start with the folder's own, else
//! from the top of the folder's volume, until a folder on the way has the watched folder's own
//! file ID (volume serial and file ID). So a target spelled in another case, with `.` or `..`, or
//! through a short (8.3) name still counts as inside when it is.
//!
//! Anything else counts as outside: a network path (`\\server\share`, `\\?\UNC\`, `\??\UNC\`) or
//! a device or volume name, none of which is ever opened; another volume (another drive letter,
//! never opened either); a `..` past the watched folder; and a link, a file, a missing part or
//! one that can't be read on the way. The last part alone may be missing, for a `commondir` that
//! names a folder not made yet ([`Last::MayMiss`]).

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf, Prefix};

use super::super::names::Key;
use super::super::run::Volume;
use super::Kind;

/// Whether the target's last part must be there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::windows::set_aside) enum Last {
    /// It must: a link's target.
    Must,
    /// It may be missing: a folder a `commondir` names that isn't made yet, watched as missing.
    MayMiss,
}

/// The key below `root` of what `target` names, written in the folder at `folder`; `None` when
/// it lies outside `root`, as the module's docs say. The key holds each folder's name as the
/// volume keeps it ([`Volume::folder_name`]), and the last part as the target spells it when it
/// isn't a folder.
pub(in crate::windows::set_aside) fn locate<V: Volume>(
    volume: &V,
    root: &Path,
    (folder, target): (&Path, &Path),
    last: Last,
) -> Option<Key> {
    let (top, words) = absolute(folder, target)?;
    let (root_top, root_words) = split(root)?;
    if !top.as_os_str().eq_ignore_ascii_case(root_top.as_os_str()) {
        return None;
    }
    let opened = volume.open_root(root).ok()?;
    if let Some(rest) = words.strip_prefix(root_words.as_slice()) {
        return walk(volume, opened, rest, last);
    }
    let root_id = volume.folder_id(&opened).ok()?;
    drop(opened);
    let mut at = volume.open_top(&root_top).ok()?;
    let mut depth = 0;
    while volume.folder_id(&at).ok()? != root_id {
        let word = words.get(depth)?;
        at = volume.open_folder(&at, word).ok()?;
        depth += 1;
    }
    walk(volume, at, words.get(depth..).unwrap_or_default(), last)
}

/// The key of `rest` below the open folder `at`, each part opened relative to the one before as
/// itself; `None` when a part on the way is a link, a file, missing or unreadable, or the last
/// is a link, or missing when it [`Last::Must`] be there.
fn walk<V: Volume>(volume: &V, at: V::Folder, rest: &[OsString], last: Last) -> Option<Key> {
    let Some((end, through)) = rest.split_last() else {
        return Some(Vec::new());
    };
    let mut key = Vec::new();
    let mut at = at;
    for word in through {
        at = volume.open_folder(&at, word).ok()?;
        key.push(volume.folder_name(&at).ok()?);
    }
    match volume.look(&at, end).ok()? {
        Some((_, Kind::Folder)) => {
            let inner = volume.open_folder(&at, end).ok()?;
            key.push(volume.folder_name(&inner).ok()?);
        }
        Some((_, Kind::Other)) => key.push(end.clone()),
        None if last == Last::MayMiss => key.push(end.clone()),
        Some((_, Kind::Link)) | None => return None,
    }
    Some(key)
}

/// `target`, written in the folder at `folder`, as the top of its volume and its plain parts
/// below it; `None` for a network path or a device or volume name (by its words, unopened), or
/// one with no drive.
fn absolute(folder: &Path, target: &Path) -> Option<(PathBuf, Vec<OsString>)> {
    if remote(target.as_os_str()) {
        return None;
    }
    // An NT name (`\??\C:\...`), as a junction's or an absolute symbolic link's target is kept.
    let mut parts = target.components();
    if let (Some(Component::RootDir), Some(Component::Normal(nt))) = (parts.next(), parts.next())
        && nt == "??"
    {
        let Some(Component::Normal(drive)) = parts.next() else {
            return None;
        };
        return Some((drive_top(drive)?, words(parts)?));
    }
    split(&folder.join(target))
}

/// `path`'s top (a drive's root, `C:\`; `/` on the tests' Unix) and its plain parts below it;
/// `None` for any other start (a network share, a device, a volume's name, no drive).
fn split(path: &Path) -> Option<(PathBuf, Vec<OsString>)> {
    let mut parts = path.components();
    let top = match parts.next()? {
        Component::Prefix(prefix) => {
            let (Prefix::Disk(letter) | Prefix::VerbatimDisk(letter)) = prefix.kind() else {
                return None;
            };
            if parts.next() != Some(Component::RootDir) {
                return None;
            }
            PathBuf::from(format!("{}:\\", char::from(letter)))
        }
        Component::RootDir if cfg!(not(windows)) => PathBuf::from("/"),
        Component::RootDir | Component::CurDir | Component::ParentDir | Component::Normal(_) => {
            return None;
        }
    };
    Some((top, words(parts)?))
}

/// The plain parts of `parts`, `.` dropped and `..` worked out by its words (one at the top
/// stays there, as Windows does).
fn words<'a>(parts: impl Iterator<Item = Component<'a>>) -> Option<Vec<OsString>> {
    let mut out = Vec::new();
    for part in parts {
        match part {
            Component::Normal(name) => out.push(name.to_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    Some(out)
}

/// The top of the drive `drive` (`C:`) names, `C:\`.
fn drive_top(drive: &OsStr) -> Option<PathBuf> {
    match drive.as_encoded_bytes() {
        [letter, b':'] if letter.is_ascii_alphabetic() => {
            Some(PathBuf::from(format!("{}:\\", char::from(*letter))))
        }
        _ => None,
    }
}

/// Whether `target`'s words name a network path, a device or a volume by its name, read as
/// Windows would read them and never opened: two separators first (`\\server\share`, `//server`),
/// or a verbatim, device or NT start (`\\?\`, `\\.\`, `\??\`) followed by anything but a drive
/// (`UNC\server`, `Volume{...}`, `GLOBALROOT`, a pipe).
pub(in crate::windows::set_aside) fn remote(target: &OsStr) -> bool {
    let sep = |byte: &u8| matches!(byte, b'\\' | b'/');
    let text = target.as_encoded_bytes();
    let rest = match text {
        [a, b'?', b'?', b, rest @ ..] if sep(a) && sep(b) => rest,
        [a, b, b'?' | b'.', c, rest @ ..] if sep(a) && sep(b) && sep(c) => rest,
        [a, b, ..] if sep(a) && sep(b) => return true,
        _ => return false,
    };
    !matches!(rest, [letter, b':', more @ ..]
        if letter.is_ascii_alphabetic() && more.first().is_none_or(sep))
}
