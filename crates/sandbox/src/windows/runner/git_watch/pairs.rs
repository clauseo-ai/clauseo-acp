//! The pairing of a folder made just now, by its creation time, with its file ID leaving another
//! place. A folder moved in from another folder comes as `FILE_ACTION_ADDED`, as `mkdir` does,
//! and an agent can set a moved folder's creation time to now before it moves it in. Its file ID
//! still leaves the place it came from: a `FILE_ACTION_REMOVED` or `FILE_ACTION_RENAMED_OLD_NAME`
//! there. The outer watches the matter, its notes folder, the agent's state folder and its temp
//! folder; every watch notes here each file ID that left ([`Pairs::note`]), and a fresh folder
//! gone before its scan counts as gone when its file ID left any other place in the last
//! [`PAIR_WINDOW`] ([`promote`]). The two records come from two watches, so the one that left
//! may be read after the folder's check: the check waits [`PAIR_WAIT`] once and looks again.
//!
//! What stays uncounted: a folder moved in from a place no watch covers that the agent can
//! write, with its creation time forged (0049).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use super::Session;
use crate::windows::set_aside::Live;

/// How long a file ID that left stays to pair with.
pub(crate) const PAIR_WINDOW: Duration = Duration::from_secs(10);
/// How many file IDs that left are kept, across every watch; the oldest go first.
pub(crate) const PAIR_MAX: usize = 4096;
/// How long a check with a fresh folder gone and unpaired waits, once, for the other watch.
pub(crate) const PAIR_WAIT: Duration = Duration::from_millis(200);

/// A file ID that left a place: when it was read, and its full path.
#[derive(Debug)]
struct Left {
    at: Duration,
    id: u64,
    path: PathBuf,
}

/// The file IDs that left lately, shared by every watch's thread.
#[derive(Debug, Default)]
pub(crate) struct Pairs(Mutex<VecDeque<Left>>);

impl Pairs {
    /// Notes each file ID in `left` (paths below `root`), read at `now` (a monotonic reading).
    pub(crate) fn note(&self, now: Duration, root: &Path, left: &[(PathBuf, u64)]) {
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        for (path, id) in left {
            kept.push_back(Left {
                at: now,
                id: *id,
                path: root.join(path),
            });
        }
        while kept.len() > PAIR_MAX
            || kept
                .front()
                .is_some_and(|old| now.saturating_sub(old.at) > PAIR_WINDOW)
        {
            kept.pop_front();
        }
    }

    /// Whether `id`, a folder made at `path` (full), left another place within [`PAIR_WINDOW`]
    /// of `now`. Its own leaving from `path` (`rm -r`, or moved back out) doesn't pair.
    pub(crate) fn paired(&self, now: Duration, id: u64, path: &Path) -> bool {
        let kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        kept.iter().any(|left| {
            left.id == id && left.path != path && now.saturating_sub(left.at) <= PAIR_WINDOW
        })
    }
}

/// One watch's share of the pairing: the shared file IDs, and the folder it watches.
#[derive(Debug)]
pub(crate) struct Paired {
    pub(crate) pairs: std::sync::Arc<Pairs>,
    pub(crate) root: PathBuf,
}

impl Paired {
    /// Notes what left the watched folder.
    pub(crate) fn note(&self, now: Duration, left: &[(PathBuf, u64)]) {
        self.pairs.note(now, &self.root, left);
    }

    fn paired(&self, now: Duration, (path, id): &(PathBuf, u64)) -> bool {
        self.pairs.paired(now, *id, &self.root.join(path))
    }
}

/// Moves each folder in `live.unpaired` whose file ID left another place (`fresh`, the
/// reported fresh folders, gives its ID) into `live.gone`, waiting [`PAIR_WAIT`] once when any
/// isn't paired yet. With no pairing (`None`), none counts.
pub(crate) fn promote(
    live: &mut Live,
    fresh: &[(PathBuf, u64)],
    paired: Option<&Paired>,
    session: &impl Session,
) {
    let unpaired = std::mem::take(&mut live.unpaired);
    let Some(paired) = paired else {
        return;
    };
    let mut waiting: Vec<&(PathBuf, u64)> = fresh
        .iter()
        .filter(|(path, _)| unpaired.contains(path))
        .collect();
    for wait in [None, Some(PAIR_WAIT)] {
        if waiting.is_empty() {
            return;
        }
        if let Some(wait) = wait {
            session.settle(wait);
        }
        let now = session.now();
        waiting.retain(|each| {
            let counted = paired.paired(now, each);
            if counted {
                live.gone.push(each.0.clone());
            }
            !counted
        });
    }
}
