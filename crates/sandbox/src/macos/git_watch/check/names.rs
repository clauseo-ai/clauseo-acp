//! The first check of each event, by its path's names alone: a `.git` anywhere in it, a
//! `HEAD` at its end, or an `objects`, `refs` or `commondir` arriving beside a `HEAD`. Such an
//! event ends the session whether or not its item is still there, so a repository renamed into
//! the matter and out again before the check still ends it. No folder is listed.

use std::path::Path;

use super::super::{ARRIVED, Event, Fs, Kind, is_git, is_head, is_name};
use super::Watch;
use crate::policy::{name_key, path_key};

/// The names that make a folder holding a `HEAD` a repository.
const SHAPE_NAMES: [&str; 3] = ["objects", "refs", "commondir"];

/// Whether `event`'s path names a git item in the matter: a `.git` at or above it, a
/// `HEAD` (unless in a launch folder that held one), or an `objects`, `refs` or `commondir`
/// that arrived beside a `HEAD`. Not at or below a launch repository the profile makes
/// read-only, nor in the volume set-aside folder. Reads the file IDs of the launch
/// repositories on the path and, for a shape name, the `HEAD` beside it; lists nothing.
pub(super) fn named<F: Fs>(watch: &Watch<F>, event: &Event) -> bool {
    let Ok(rel) = event.path.strip_prefix(&watch.matter) else {
        return false;
    };
    let mut at = watch.matter.clone();
    let mut key = path_key(&watch.matter);
    for name in rel {
        at.push(name);
        key.push(name_key(name));
        if watch.kept(&watch.repos, &key, &at) || watch.skip.contains(&key) {
            return false;
        }
        if is_git(name) {
            return true;
        }
    }
    let (Some(name), Some(dir)) = (event.path.file_name(), event.path.parent()) else {
        return false;
    };
    if is_head(name) {
        return !watch.kept(&watch.heads, &path_key(dir), dir);
    }
    event.flags & ARRIVED != 0
        && SHAPE_NAMES.iter().any(|want| is_name(name, want))
        && has_head(&watch.fs, dir)
}

/// Whether folder `dir` holds a `HEAD` file or link, read by its name without a listing.
fn has_head<F: Fs>(fs: &F, dir: &Path) -> bool {
    fs.meta(&dir.join("HEAD"))
        .is_some_and(|meta| matches!(meta.kind, Kind::File | Kind::Link))
}
