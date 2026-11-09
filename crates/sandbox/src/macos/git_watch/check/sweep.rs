//! The 30 s pass: the backstop for events the stream lost without saying so (decision record
//! 0038). It keeps the matter's folders with each one's file ID and [`Stamp`] (from the
//! launch's walk, then from its own listings) and, in each pass, reads each folder's again and
//! lists only those whose ID or stamp changed: their own finds go to the session's end as an
//! event's do, and their new subfolders join the list, to be listed in turn. A pass stops when
//! the runner's cap says so and the next goes on from there, so one pass never takes more than
//! about 250 ms of the watch's thread, however big the matter.

use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::super::{Fs, Kind, Stamp, is_git};
use super::{Found, Id, Watch};
use crate::policy::path_key;

/// One folder of the pass, as it was when last read: `None` until it is first read.
struct Folder {
    path: PathBuf,
    id: Option<Id>,
    stamp: Option<Stamp>,
}

/// The pass's folders, by path key once each, and the one it reads next.
#[derive(Default)]
pub(crate) struct Sweep {
    folders: Vec<Folder>,
    known: HashSet<PathBuf>,
    cursor: usize,
}

impl Sweep {
    /// Adds folder `path` as it is now (`None`: unread, so it is listed when reached), once.
    pub(crate) fn add(&mut self, path: &Path, id: Option<Id>, stamp: Option<Stamp>) {
        if self.known.insert(path_key(path)) {
            self.folders.push(Folder {
                path: path.to_owned(),
                id,
                stamp,
            });
        }
    }
}

/// Runs `sweep` from its cursor until `out_of_time` (asked before each folder) or the end,
/// adding what `watch` finds to `found`. Returns whether the pass reached its end.
pub(super) fn stretch<F: Fs>(
    watch: &Watch<F>,
    sweep: &mut Sweep,
    out_of_time: &dyn Fn() -> bool,
    found: &mut Found,
) -> bool {
    loop {
        if sweep.cursor >= sweep.folders.len() {
            sweep.cursor = 0;
            return true;
        }
        if out_of_time() {
            return false;
        }
        if step(watch, sweep, found) {
            sweep.cursor += 1;
        }
    }
}

/// Reads the pass's folder at its cursor in `sweep`, and lists it when it changed, adding
/// what `watch` finds there to `found`, the folder itself when it can't be listed. Returns
/// false when the folder left the pass: gone, no longer a folder, a launch repository or the
/// volume set-aside folder, or on a network volume.
fn step<F: Fs>(watch: &Watch<F>, sweep: &mut Sweep, found: &mut Found) -> bool {
    let at = sweep.cursor;
    let Some(path) = sweep.folders.get(at).map(|folder| folder.path.clone()) else {
        return false;
    };
    let key = path_key(&path);
    let dated = watch
        .fs
        .dated(&path)
        .filter(|(meta, _)| meta.kind == Kind::Dir && !watch.passed(&key, &path))
        .filter(|_| watch.fs.local(&path));
    let (Some((meta, stamp)), Some(folder)) = (dated, sweep.folders.get_mut(at)) else {
        sweep.known.remove(&key);
        sweep.folders.remove(at);
        return false;
    };
    if folder.id == Some(meta.id) && folder.stamp == Some(stamp) {
        return true;
    }
    folder.id = Some(meta.id);
    folder.stamp = Some(stamp);
    let entries = match watch.fs.list(&path) {
        Ok(entries) => entries,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::Unsupported) => {
            return true;
        }
        Err(error) => {
            tracing::warn!(dir = %path.display(), %error, "the git watch can't list a folder");
            found.unreadable.push((path, error.to_string()));
            return true;
        }
    };
    (watch.progress)();
    if watch.folder(&path, meta, &entries, &mut found.items) {
        for (name, kind) in &entries {
            if *kind == Kind::Dir && !is_git(name) {
                sweep.add(&path.join(name), None, None);
            }
        }
    }
    true
}
