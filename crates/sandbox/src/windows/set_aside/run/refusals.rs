//! What a restore refuses: a git folder or an item in one ([`holds_git`], [`holds_repo`]),
//! an item that was git's in a folder a launch `commondir` names, and a link, at any depth of
//! the item, that leads out of the folder it goes back to. Put back, the first would be read by
//! git as the lawyer's (its `config` and `hooks` run code as the lawyer); the second would lead
//! git, or anything else, outside.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use clauseo_protocol::MatterId;

use super::super::Entry;
use super::super::files::{Place, Record};
use super::super::names::Key;
use super::super::repo_names::{COMMON_NAMES, is_commondir, is_git, is_head, opens_as};
use super::super::scan::{COMMON_KEY, Kind, Last, locate, same_key};
use super::Volume;

/// What git reads from a `commondir`'s folder that runs code as the lawyer: refused by its name
/// alone, wherever it was, since only a folder a `commondir` names sets one aside as itself.
const RUNS_CODE: [&str; 2] = ["config", "hooks"];

/// The folders a launch `commondir` names, by their keys below the root, as the matter's and
/// the notes folder's records hold them (`:common` keys). A record that can't be read adds none:
/// the check by name still holds.
/// The caller holds the matter's lock.
pub(super) fn common_targets(data_dir: &Path, matter: &MatterId) -> Vec<Key> {
    let mut targets = Vec::new();
    for each in [Place::new(data_dir, matter), Place::notes(data_dir, matter)] {
        match each.read_record() {
            Ok(Some(record)) => targets.extend(targets_of(&record)),
            Ok(None) => {}
            Err(failure) => tracing::debug!(%failure, "a set-aside record for the restore's check"),
        }
    }
    targets
}

/// The `:common` keys in `record`, without their first part.
fn targets_of(record: &Record) -> impl Iterator<Item = Key> + '_ {
    record
        .names
        .keys()
        .filter(|key| key.first().is_some_and(|first| first == COMMON_KEY))
        .map(|key| key.get(1..).unwrap_or_default().to_vec())
}

/// Whether `entry`'s item was git's in a folder a launch `commondir` names: not a link (a link
/// is checked by [`leads_out`]), and its own name opens as `config` or `hooks`, or it is one of
/// `targets` (made while missing), or one of [`COMMON_NAMES`] in one.
pub(super) fn from_common(entry: &Entry, targets: &[Key]) -> bool {
    if is_link(&entry.to) {
        return false;
    }
    let Some(own) = entry.from.file_name() else {
        return true;
    };
    if RUNS_CODE.iter().any(|name| opens_as(own, name)) {
        return true;
    }
    let key: Vec<OsString> = entry
        .within
        .iter()
        .cloned()
        .chain([own.to_owned()])
        .collect();
    let common = COMMON_NAMES.iter().any(|name| opens_as(own, name));
    targets.iter().any(|target| {
        let parent = key.get(..key.len().saturating_sub(1)).unwrap_or_default();
        (!target.is_empty() && same_key(&key, target)) || (common && same_key(parent, target))
    })
}

/// Whether `entry`'s item is a link, or holds one at any depth, whose target leads outside the
/// matter's (or notes) folder it came from, or can't be read. Each target is read from the
/// link itself, never followed, worked out from its words relative to the folder the link goes
/// back to, and told inside by identity, as the live watch tells it (`scan/locate.rs`): a
/// target spelled in another case, or with `..`, still counts as inside when it is. A target
/// inside the item itself isn't back yet, so it counts as outside. The item's folders are opened
/// as the scan opens them, never through a link; one that can't be opened or listed counts as
/// holding a link out. A file is `false`.
pub(super) fn leads_out<V: Volume>(volume: &V, entry: &Entry) -> bool {
    let (Some(folder), Some(root)) = (entry.from.parent(), root_of(entry)) else {
        return true;
    };
    let out = |at: &Path, target: &Path| locate(volume, &root, (at, target), Last::Must).is_none();
    let Ok(meta) = std::fs::symlink_metadata(&entry.to) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return std::fs::read_link(&entry.to).map_or(true, |target| out(folder, &target));
    }
    if !meta.is_dir() {
        return false;
    }
    let Ok(top) = volume.open_root(&entry.to) else {
        return true;
    };
    let mut stack: Vec<(V::Folder, PathBuf)> = vec![(top, entry.from.clone())];
    while let Some((at, back)) = stack.pop() {
        let Ok(listed) = volume.list(&at) else {
            return true;
        };
        for child in listed {
            match child.kind {
                Kind::Link => {
                    let Ok(target) = volume.link_target(&at, &child.name) else {
                        return true;
                    };
                    if out(&back, &target) {
                        return true;
                    }
                }
                Kind::Folder => match volume.open_folder(&at, &child.name) {
                    Ok(inner) => stack.push((inner, back.join(&child.name))),
                    Err(_) => return true,
                },
                Kind::Other => {}
            }
        }
    }
    false
}

/// The folder `entry`'s item was set aside from: its `from` less its name and the folders
/// `within` names. The walk's item has none, so its own folder counts, which only narrows.
fn root_of(entry: &Entry) -> Option<PathBuf> {
    let mut root = entry.from.parent()?;
    for _ in &entry.within {
        root = root.parent()?;
    }
    Some(root.to_owned())
}

/// Whether `path` itself is a link (a junction or a symbolic link), never what it names.
fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// Whether `entry`'s item is a `.git` or sits inside one: its own name, a folder it was in below
/// the matter, or any part of the path it would go back to opens as `.git` ([`is_git`]). The
/// restore's folder is opened by that path and must have it as its final path (`move_back`), so
/// no short name or link can land it inside a `.git` the words don't name.
pub(super) fn holds_git(entry: &Entry) -> bool {
    let in_path = entry.from.components().any(|part| match part {
        Component::Normal(name) => is_git(name),
        Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir => {
            false
        }
    });
    in_path || is_git(&entry.name) || entry.within.iter().any(|part| is_git(part))
}

/// Whether `entry`'s item, where it is now, is or holds at any depth a `HEAD` (a file or a
/// link), a `commondir` or a `.git`: a bare repository or a worktree's git folder set aside
/// whole, which put back would be a git folder git trusts. Its folders are opened as the scan
/// opens them, never through a link; one that can't be opened or listed counts as holding one.
pub(super) fn holds_repo(volume: &impl Volume, entry: &Entry) -> bool {
    let named = |name: &OsStr, kind: Kind| {
        is_git(name) || (kind != Kind::Folder && (is_head(name) || is_commondir(name)))
    };
    let own = entry.from.file_name().unwrap_or(&entry.name);
    let Ok(meta) = std::fs::symlink_metadata(&entry.to) else {
        return named(own, Kind::Other);
    };
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return named(own, Kind::Other);
    }
    let Ok(top) = volume.open_root(&entry.to) else {
        return true;
    };
    let mut stack = vec![top];
    while let Some(folder) = stack.pop() {
        let Ok(listed) = volume.list(&folder) else {
            return true;
        };
        for child in listed {
            if named(&child.name, child.kind) {
                return true;
            }
            if child.kind == Kind::Folder {
                match volume.open_folder(&folder, &child.name) {
                    Ok(inner) => stack.push(inner),
                    Err(_) => return true,
                }
            }
        }
    }
    is_git(own)
}
