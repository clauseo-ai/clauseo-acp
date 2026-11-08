//! What a restore refuses besides a git folder (`holds_git`, `holds_repo`): an item that was
//! git's in a folder a launch `commondir` names, and a link that leads out of the folder it
//! goes back to. Put back, the first would be read by git as the lawyer's (its `config` and
//! `hooks` run code as the lawyer); the second would lead git, or anything else, outside.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::super::Entry;
use super::super::files::{Place, Record};
use super::super::names::Key;
use super::super::repo_names::{COMMON_NAMES, opens_as};
use super::super::scan::{COMMON_KEY, plain, same_key};

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

/// Whether `entry`'s item is a link whose target, read by its words and worked out from the
/// folder it goes back to, leads outside the matter's (or notes) folder it came from, or can't
/// be read: never opened, as the live watch's check does. Not a link is `false`.
pub(super) fn leads_out(entry: &Entry) -> bool {
    if !is_link(&entry.to) {
        return false;
    }
    let Ok(target) = std::fs::read_link(&entry.to) else {
        return true;
    };
    let (Some(folder), Some(root)) = (entry.from.parent(), root_of(entry)) else {
        return true;
    };
    let (full, root) = (plain(&folder.join(target)), plain(&root));
    !(full.len() >= root.len()
        && full
            .iter()
            .zip(&root)
            .all(|(a, b)| a.eq_ignore_ascii_case(b)))
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
