//! The pairing of a folder made just now, by its creation time, with its file ID leaving another
//! place. A folder moved in from another folder comes as `FILE_ACTION_ADDED`, as `mkdir` does,
//! and an agent can set a moved folder's creation time to now before it moves it in. Its file ID
//! still leaves the place it came from: a `FILE_ACTION_REMOVED` or `FILE_ACTION_RENAMED_OLD_NAME`
//! there. The outer watches the matter, its notes folder, the agent's state folder and its temp
//! folder; every watch notes here each folder's file ID that left ([`Pairs::note`]), and a fresh
//! folder gone before its scan counts as gone when its file ID left any other place within
//! [`PAIR_WINDOW`] of its check, in either order: one that left first is kept that long, and a
//! fresh folder not paired yet is held that long ([`Pairs::hold`]) for a leaving record read
//! later, by another watch's thread. What paired is taken, and counted, by the folder's own watch
//! on its next record or its next idle tick (`os.rs`), and at its stop ([`Pairs::take`],
//! [`Pairs::finish`]). A held folder nothing paired by the window's end is left to the
//! creation-time rule, which says made just now: uncounted.
//!
//! The pairing fails closed when it loses track. A file ID under [`PAIR_WINDOW`] old pushed out
//! past [`PAIR_MAX`], a held folder pushed out the same way, or any watch's overflow, marks the
//! pairing lost for [`PAIR_WINDOW`] ([`Pairs::lose`]); a pairing watch that can't start or breaks
//! marks it lost for the session ([`Pairs::lose_for_good`]). While it is lost, every fresh folder
//! gone and unpaired counts, the ones held when it was lost included.
//!
//! The file ID is the extended record's 64-bit `FileId`, read whole (`records.rs`). On NTFS it is
//! the file reference number: the MFT record's 48-bit number and its 16-bit sequence number, which
//! NTFS bumps each time it reuses the record, so a folder made where another was just deleted
//! doesn't take its ID within the window. On `ReFS` the 64-bit ID isn't promised unique: a
//! false match there only counts more.
//!
//! What stays uncounted: a folder moved in from a place no watch covers that the agent can
//! write, with its creation time forged (0049).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::windows::set_aside::Live;

/// How long a file ID that left stays to pair with, and a fresh folder gone is held for one;
/// also how long the pairing stays lost once it lost track.
pub(crate) const PAIR_WINDOW: Duration = Duration::from_secs(10);
/// How many file IDs that left are kept, across every watch, and as many fresh folders held; the
/// oldest go first, and one going within [`PAIR_WINDOW`] loses the pairing.
pub(crate) const PAIR_MAX: usize = 4096;

/// A folder's file ID that left a place: when it was read, and its full path.
#[derive(Debug)]
struct Left {
    at: Duration,
    id: u64,
    path: PathBuf,
}

/// A fresh folder gone before its check and not paired yet: when it was checked, its file ID,
/// the folder its watch covers and its path below it.
#[derive(Debug)]
struct Held {
    at: Duration,
    id: u64,
    root: PathBuf,
    path: PathBuf,
}

impl Held {
    /// Whether a file ID `id` leaving `path` (full) at `now` pairs with this one.
    fn pairs_with(&self, now: Duration, id: u64, path: &Path) -> bool {
        self.id == id
            && self.root.join(&self.path) != path
            && now.saturating_sub(self.at) <= PAIR_WINDOW
    }
}

/// What the watches share, under one lock.
#[derive(Debug, Default)]
struct Kept {
    left: VecDeque<Left>,
    held: VecDeque<Held>,
    /// Fresh folders that count gone (paired, or held while the pairing was lost), for their own
    /// watch to take: the folder it covers, and the path below it.
    counted: Vec<(PathBuf, PathBuf)>,
    /// Until when the pairing is lost.
    lost_until: Option<Duration>,
    /// Whether it is lost for the session.
    lost_for_good: bool,
}

impl Kept {
    fn lost(&self, now: Duration) -> bool {
        self.lost_for_good || self.lost_until.is_some_and(|until| now <= until)
    }

    fn lose(&mut self, now: Duration) {
        let until = now.saturating_add(PAIR_WINDOW);
        self.lost_until = Some(self.lost_until.map_or(until, |was| was.max(until)));
        self.count_held();
    }

    /// Counts every folder held: the pairing lost track while they waited.
    fn count_held(&mut self) {
        let held = std::mem::take(&mut self.held);
        self.counted
            .extend(held.into_iter().map(|held| (held.root, held.path)));
    }

    fn paired(&self, now: Duration, id: u64, path: &Path) -> bool {
        self.left.iter().any(|left| {
            left.id == id && left.path != path && now.saturating_sub(left.at) <= PAIR_WINDOW
        })
    }
}

/// The file IDs that left lately, the fresh folders held, and whether the pairing lost track:
/// shared by every watch's thread.
#[derive(Debug, Default)]
pub(crate) struct Pairs(Mutex<Kept>);

impl Pairs {
    fn kept(&self) -> MutexGuard<'_, Kept> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Notes each folder's file ID in `left` (paths below `root`), read at `now` (a monotonic
    /// reading): a held folder it pairs with counts. One pushed out past [`PAIR_MAX`] while
    /// under [`PAIR_WINDOW`] old loses the pairing ([`Pairs::lose`]).
    pub(crate) fn note(&self, now: Duration, root: &Path, left: &[(PathBuf, u64)]) {
        let mut kept = self.kept();
        for (path, id) in left {
            let path = root.join(path);
            let (paired, held): (VecDeque<Held>, VecDeque<Held>) = std::mem::take(&mut kept.held)
                .into_iter()
                .partition(|held| held.pairs_with(now, *id, &path));
            kept.held = held;
            kept.counted
                .extend(paired.into_iter().map(|held| (held.root, held.path)));
            kept.left.push_back(Left {
                at: now,
                id: *id,
                path,
            });
        }
        while let Some(old) = kept.left.front() {
            let young = now.saturating_sub(old.at) <= PAIR_WINDOW;
            if kept.left.len() <= PAIR_MAX && young {
                break;
            }
            if young {
                kept.lose(now);
            }
            kept.left.pop_front();
        }
    }

    /// Whether `id`, a folder made at `path` (full), left another place within [`PAIR_WINDOW`]
    /// of `now`. Its own leaving from `path` (`rm -r`, or moved back out) doesn't pair.
    #[cfg(test)]
    pub(crate) fn paired(&self, now: Duration, id: u64, path: &Path) -> bool {
        self.kept().paired(now, id, path)
    }

    /// A fresh folder `path` (below `root`), file ID `id`, gone at its check at `now` and
    /// unpaired: counted when its file ID already left another place or the pairing is lost,
    /// else held for [`PAIR_WINDOW`]. Either way its watch takes it ([`Pairs::take`]).
    pub(crate) fn hold(&self, now: Duration, root: &Path, path: &Path, id: u64) {
        let mut kept = self.kept();
        if kept.paired(now, id, &root.join(path)) || kept.lost(now) {
            kept.counted.push((root.to_owned(), path.to_owned()));
            return;
        }
        kept.held.push_back(Held {
            at: now,
            id,
            root: root.to_owned(),
            path: path.to_owned(),
        });
        // Those past the window are left to the creation-time rule, as [`Pairs::take`] leaves
        // them; one pushed out within it would lose track.
        kept.held
            .retain(|held| now.saturating_sub(held.at) <= PAIR_WINDOW);
        if kept.held.len() > PAIR_MAX {
            kept.lose(now);
        }
    }

    /// Marks the pairing lost until [`PAIR_WINDOW`] past `now`: a watch overflowed, or a file
    /// ID still in its window was pushed out. Every folder held counts, as does every fresh
    /// folder gone and unpaired until then.
    pub(crate) fn lose(&self, now: Duration) {
        self.kept().lose(now);
    }

    /// Marks the pairing lost for the session: a pairing watch couldn't start, or broke.
    /// Whether this is the session's first such loss, so the lawyer is told once.
    pub(crate) fn lose_for_good(&self) -> bool {
        let mut kept = self.kept();
        let first = !std::mem::replace(&mut kept.lost_for_good, true);
        kept.count_held();
        first
    }

    /// Whether the pairing is lost at `now`.
    #[cfg(test)]
    pub(crate) fn lost(&self, now: Duration) -> bool {
        self.kept().lost(now)
    }

    /// What counts gone of the fresh folders `root`'s watch held, by their paths below it. Those
    /// held past [`PAIR_WINDOW`] with nothing paired are dropped: the creation-time rule says
    /// they were made just now, and a lost pairing in their window counted them already.
    pub(crate) fn take(&self, now: Duration, root: &Path) -> Vec<PathBuf> {
        let mut kept = self.kept();
        kept.held
            .retain(|held| held.root != root || now.saturating_sub(held.at) <= PAIR_WINDOW);
        Self::counted(&mut kept, root)
    }

    /// The same at `root`'s watch's stop: what counts, and every folder still held dropped, by
    /// the creation-time rule.
    pub(crate) fn finish(&self, root: &Path) -> Vec<PathBuf> {
        let mut kept = self.kept();
        kept.held.retain(|held| held.root != root);
        Self::counted(&mut kept, root)
    }

    fn counted(kept: &mut Kept, root: &Path) -> Vec<PathBuf> {
        let (taken, rest) = std::mem::take(&mut kept.counted)
            .into_iter()
            .partition(|(each, _)| each == root);
        kept.counted = rest;
        taken.into_iter().map(|(_, path)| path).collect()
    }
}

/// One watch's share of the pairing: the shared file IDs, and the folder it watches.
#[derive(Debug)]
pub(crate) struct Paired {
    pub(crate) pairs: Arc<Pairs>,
    pub(crate) root: PathBuf,
}

impl Paired {
    /// Notes the folders that left the watched folder.
    pub(crate) fn note(&self, now: Duration, left: &[(PathBuf, u64)]) {
        self.pairs.note(now, &self.root, left);
    }

    /// See [`Pairs::lose`].
    pub(crate) fn lose(&self, now: Duration) {
        self.pairs.lose(now);
    }

    /// What counts gone of the fresh folders this watch held ([`Pairs::take`]).
    pub(crate) fn take(&self, now: Duration) -> Vec<PathBuf> {
        self.pairs.take(now, &self.root)
    }

    /// The same at the watch's stop ([`Pairs::finish`]).
    pub(crate) fn finish(&self) -> Vec<PathBuf> {
        self.pairs.finish(&self.root)
    }
}

/// Hands each folder in `live.unpaired` (`fresh`, the reported fresh folders, gives its file ID)
/// to the pairing at `now` ([`Pairs::hold`]), then moves into `live.gone` what counts of every
/// folder this watch held. With no pairing (`None`), none counts.
pub(crate) fn promote(
    live: &mut Live,
    fresh: &[(PathBuf, u64)],
    paired: Option<&Paired>,
    now: Duration,
) {
    let unpaired = std::mem::take(&mut live.unpaired);
    let Some(paired) = paired else {
        return;
    };
    for (path, id) in fresh.iter().filter(|(path, _)| unpaired.contains(path)) {
        paired.pairs.hold(now, &paired.root, path, *id);
    }
    live.gone.extend(paired.take(now));
}
