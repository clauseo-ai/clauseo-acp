//! Taking a checkpoint: walk the folder against the stat cache, hash only what changed, build
//! the tree, diff it against the last checkpoint, and commit it unless nothing changed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use clauseo_protocol::{
    CheckpointId, CheckpointKind, FileChange, MatterPath, SkippedFile, SkippedFileReason, Timestamp,
};

use crate::diff::{DiffFile, TurnStart, diff_by, flag};
use crate::first_read::{FirstReads, StepReads, first_reads};
use crate::history::{GixStore, Hasher, HistoryStore, Oid, TreeFile};
use crate::no_download::NoDownload;
use crate::skip_record::{self, SkipRecord, SkipsRef};
use crate::stat::{CacheEntry, EntryState, Stat, StatCache, find_by_path};
use crate::types::{Checkpoint, CheckpointOutcome, CommitId, Progress, Settings, TimelineEntry};
use crate::{Error, trailers};
pub(crate) use timeline::Timeline;
pub(crate) use walk_folder::OnHeld;
use walk_folder::walk_folder;

/// The history of one matter as this process holds it: the repository, the stat cache and the
/// timeline rebuilt from the trailers.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) history: GixStore,
    pub(crate) cache: StatCache,
    /// The tree the cache describes; when it is not the tip's, the cache is rebuilt.
    cache_tree: Option<Oid>,
    pub(crate) timeline: Timeline,
}

/// What an after-turn checkpoint's turn named: the paths its tool calls named, and the paths
/// its shell commands wrote (`crate::Matter::after_turn_named`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Named<'a> {
    pub(crate) paths: &'a BTreeSet<MatterPath>,
    pub(crate) commands: &'a BTreeSet<String>,
}

/// What a checkpoint is for: its trailers and, after a turn, the paths the agent's tool calls
/// named.
pub(crate) struct Spec<'a> {
    pub(crate) entry: TimelineEntry,
    pub(crate) at: Timestamp,
    /// `Some` for an `after-turn` checkpoint: a change to a path the turn did not name may
    /// hold the lawyer's edits.
    pub(crate) tool_paths: Option<Named<'a>>,
}

/// A walk's result: the new cache entries, the skipped files worth listing, whether any file
/// was read, and the files another program held so they could not be read.
pub(crate) struct Walked {
    entries: Vec<CacheEntry>,
    skipped: Vec<SkippedFile>,
    /// A file was read: if only racily clean files were, the entries may equal the cache's and
    /// the cache must still be written, so its time moves past them.
    read_any: bool,
    /// The walk's size cap, in bytes: tells why a file the cache has as skipped was skipped.
    cap: u64,
    /// The folders that did not list, relative to the matter folder.
    unread: Vec<String>,
    /// Why each file this walk skipped was skipped, by path; one kept from the cache under a
    /// folder that did not list has none.
    reasons: BTreeMap<String, SkippedFileReason>,
    /// The files whose read failed because another program holds them (Windows: a sharing or
    /// lock violation), sorted by path bytes; only a walk with [`OnHeld::Report`] has any.
    /// Their entries are the cache's, as it last knew them, so such a walk is never committed.
    held: Vec<MatterPath>,
}

impl Walked {
    /// The folder as walked: one entry per file, sorted by path bytes.
    pub(crate) fn entries(&self) -> &[CacheEntry] {
        &self.entries
    }

    /// The files another program held so the walk could not read them, sorted by path bytes.
    pub(crate) fn held(&self) -> &[MatterPath] {
        &self.held
    }
}

/// One file of a tree, with its size.
#[derive(Debug, Clone)]
pub(crate) struct SizedFile {
    pub(crate) path: MatterPath,
    pub(crate) blob: Oid,
    pub(crate) size: u64,
}

impl Core {
    /// The core of a matter whose repository is `history`; reads the timeline and the cache.
    pub(crate) fn load(history: GixStore) -> Result<Self, Error> {
        let records = history.timeline()?;
        let timeline = Timeline::from_records(&records);
        let cache = history.load_cache()?.unwrap_or_default();
        let mut core = Self {
            history,
            cache,
            cache_tree: None,
            timeline,
        };
        core.check_cache()?;
        Ok(core)
    }

    /// Makes the cache describe the tip's tree: kept if its blobs match the tree, otherwise
    /// rebuilt from the tree with empty stats, so every file is read once.
    pub(crate) fn check_cache(&mut self) -> Result<(), Error> {
        let Some((_, tree)) = self.timeline.tip() else {
            self.cache = StatCache::default();
            self.cache_tree = None;
            return Ok(());
        };
        if self.cache_tree == Some(tree) {
            return Ok(());
        }
        if let Some(rebuilt) = self.rebuilt_cache(tree)? {
            self.cache = rebuilt;
        }
        self.cache_tree = Some(tree);
        Ok(())
    }

    /// `None` when the cache's blobs match tree `tree`; otherwise the cache rebuilt from the tree
    /// with empty stats.
    fn rebuilt_cache(&self, tree: Oid) -> Result<Option<StatCache>, Error> {
        let files = self.history.read_tree(tree)?;
        let cached = self
            .cache
            .entries
            .iter()
            .filter_map(|entry| entry.state.blob().map(|blob| (entry.path.as_str(), blob)));
        if cached.eq(files.iter().map(|file| (file.path.as_str(), file.blob))) {
            return Ok(None);
        }
        tracing::info!("the stat cache does not match the timeline's tip; rebuilding it");
        let mut entries = Vec::with_capacity(files.len());
        for file in files {
            let path = MatterPath::new(&file.path).map_err(|err| Error::history("tree", err))?;
            let size = self.history.blob_len(file.blob)?;
            let stat = Stat {
                size: u32::try_from(size).unwrap_or(u32::MAX),
                ..Stat::default()
            };
            entries.push(CacheEntry {
                path,
                stat,
                state: EntryState::Read(file.blob),
            });
        }
        Ok(Some(StatCache {
            entries,
            written_at: None,
        }))
    }

    /// Walks `folder` and hashes and stores every file the cache cannot vouch for. `Ok(None)`
    /// when cancelled. A file another program holds fails the walk. See [`walk_folder`].
    pub(crate) fn scan_folder(
        &mut self,
        folder: &Path,
        settings: &Settings,
        cancel: Option<&AtomicBool>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Option<Walked>, Error> {
        self.scan(folder, settings, cancel, progress, OnHeld::Fail)
    }

    /// [`Core::scan_folder`], with a file another program holds failing the walk or listed in
    /// [`Walked::held`], by `on_held`.
    pub(crate) fn scan(
        &mut self,
        folder: &Path,
        settings: &Settings,
        cancel: Option<&AtomicBool>,
        progress: &mut dyn FnMut(Progress),
        on_held: OnHeld,
    ) -> Result<Option<Walked>, Error> {
        self.check_cache()?;
        let history = &mut self.history;
        let mut store = |path: &Path, len: u64, cap: u64, guard: &NoDownload<'_>| {
            history.store_file(path, len, cap, guard)
        };
        walk_folder(
            &self.cache,
            folder,
            settings,
            cancel,
            progress,
            &mut store,
            on_held,
        )
    }

    /// The folder as a checkpoint would see it now, sorted by path, without storing anything or
    /// writing the cache: every file the cache cannot vouch for is hashed only. Takes `&self`,
    /// so a preview or a compare never needs the writer. A file another program holds fails
    /// the walk or is listed, by `on_held`.
    ///
    /// When the cache on disk was written for another tree than the tip's (after an open, until
    /// the next checkpoint), each call rebuilds it from the tip's tree, since `&self` cannot
    /// keep the result; the next checkpoint writes the cache and ends that.
    pub(crate) fn snapshot(
        &self,
        folder: &Path,
        settings: &Settings,
        on_held: OnHeld,
    ) -> Result<Walked, Error> {
        let rebuilt = match self.timeline.tip() {
            None => Some(StatCache::default()),
            Some((_, tree)) if self.cache_tree != Some(tree) => self.rebuilt_cache(tree)?,
            Some(_) => None,
        };
        let cache = rebuilt.as_ref().unwrap_or(&self.cache);
        let mut hasher = Hasher::new();
        let mut hash = |path: &Path, len: u64, cap: u64, guard: &NoDownload<'_>| {
            hasher.hash(path, len, cap, guard)
        };
        walk_folder(
            cache,
            folder,
            settings,
            None,
            &mut |_| {},
            &mut hash,
            on_held,
        )?
        .ok_or_else(|| Error::history("walk", "cancelled without a cancel flag"))
    }

    /// Commits `walked` as checkpoint `spec`, or reuses the last checkpoint when the folder
    /// equals the tip's tree and that is the last checkpoint's tree (never for the root
    /// checkpoint), and writes the cache. The parent is the tip, checkpoint or not.
    pub(crate) fn commit(&mut self, walked: Walked, spec: &Spec<'_>) -> Result<Checkpoint, Error> {
        let Walked {
            entries,
            skipped,
            read_any,
            cap,
            unread,
            reasons,
            held,
        } = walked;
        // A held file's entry is only what the cache last knew: never committed as the folder.
        if !held.is_empty() {
            return Err(Error::FileLocked { paths: held });
        }
        let old_tree = tree_of(&self.cache.entries);
        let new_tree = tree_of(&entries);
        let over_cap: BTreeSet<&str> = entries
            .iter()
            .filter(|entry| entry.state == EntryState::Skipped)
            .map(|entry| entry.path.as_str())
            .collect();
        // What the folder holds but does not version, as the last commit recorded it (or, for a
        // tip from before records, as the cache knows it) and as this walk found it.
        let (tip_record, tip_recorded) = self.tip_record(cap);
        let record = SkipRecord::of_walk(&entries, &unread, &reasons, cap);
        // A file read for the first time was there before: a baseline is no change, and a
        // changed one is never taken for the new name of a renamed file.
        let versioned =
            |path: &str| find_by_path(&old_tree, path, |file| file.path.as_str()).is_some();
        let mut reads = first_reads(&tip_record, &entries, &versioned);
        let mut changes = diff_reads(&old_tree, &new_tree, &over_cap, &reads)?;
        let head = self.timeline.head();
        let tip = self.timeline.tip();
        let same_tree = old_tree.len() == new_tree.len()
            && old_tree
                .iter()
                .zip(&new_tree)
                .all(|(old, new)| old.path == new.path && old.blob == new.blob);
        let tip_is_head = head.map(|(_, _, tree)| tree) == tip.map(|(_, tree)| tree);
        // Reused only when the commit would record the same: an undo reads what was there at a
        // turn's start from the checkpoint it started at.
        let same_record = tip_recorded && tip_record == record;
        if let (Some((last, last_commit, _)), true) =
            (head, same_tree && tip_is_head && same_record)
        {
            if read_any || entries != self.cache.entries {
                let mut cache = StatCache {
                    entries,
                    written_at: self.cache.written_at,
                };
                self.history.save_cache(&mut cache)?;
                self.cache = cache;
            }
            return Ok(Checkpoint {
                outcome: CheckpointOutcome::Reused(last),
                commit: CommitId::of(last_commit),
                changes,
                skipped,
            });
        }
        if self.timeline.contains(spec.entry.id) {
            return Err(Error::DuplicateCheckpoint { id: spec.entry.id });
        }
        if let Some(named) = spec.tool_paths {
            let start = TurnStart {
                commands: named.commands,
                record: tip_recorded.then_some(&tip_record),
                tree: &old_tree,
                reads: &reads,
                entries: &entries,
                at: spec.at,
            };
            changes = changes
                .into_iter()
                .map(|change| flag(change, named.paths, &start))
                .collect();
        }
        let files: Vec<TreeFile> = new_tree
            .iter()
            .map(|file| TreeFile {
                path: file.path.as_str().to_owned(),
                blob: file.blob,
            })
            .collect();
        let tree = self.history.write_tree(&files)?;
        let count = if spec.entry.kind == CheckpointKind::Import {
            files.len()
        } else {
            changes.len()
        };
        reads.skips = if record.is_empty() {
            SkipsRef::Empty
        } else {
            SkipsRef::Blob(self.history.write_blob(record.encode().as_bytes())?)
        };
        let message = trailers::message(&spec.entry, count, &reads);
        let parent = tip.map(|(commit, _)| commit);
        let seconds = spec.at.as_millis().div_euclid(1000);
        let commit = self.history.write_commit(tree, parent, &message, seconds)?;
        let mut cache = StatCache {
            entries,
            written_at: self.cache.written_at,
        };
        if let Err(err) = self.history.publish(commit, parent, &mut cache) {
            // The ref may or may not have moved: read it again before the next checkpoint.
            if let Err(reload) = self.reload() {
                tracing::warn!(code = %reload.code(), "the timeline did not read after a failed publish");
            }
            return Err(err);
        }
        self.cache = cache;
        self.cache_tree = Some(tree);
        self.timeline.push(spec.entry, commit, tree, reads);
        Ok(Checkpoint {
            outcome: CheckpointOutcome::Created(spec.entry.id),
            commit: CommitId::of(commit),
            changes,
            skipped,
        })
    }

    /// Walks and commits in one go; never cancelled; a held file fails it.
    pub(crate) fn take(
        &mut self,
        folder: &Path,
        settings: &Settings,
        spec: &Spec<'_>,
    ) -> Result<Checkpoint, Error> {
        let walked = self
            .scan_folder(folder, settings, None, &mut |_| {})?
            .ok_or_else(|| Error::history("walk", "cancelled without a cancel flag"))?;
        self.commit(walked, spec)
    }

    /// Reads the timeline again after a failed publish; the cache is rebuilt if it no longer
    /// matches.
    fn reload(&mut self) -> Result<(), Error> {
        let records = self.history.timeline()?;
        self.timeline = Timeline::from_records(&records);
        Ok(())
    }

    /// The files of checkpoint `id`'s tree, without sizes, sorted by path.
    pub(crate) fn tree_files(&self, id: CheckpointId) -> Result<Vec<VersionEntry>, Error> {
        let tree = self.timeline.tree(id)?;
        version_entries(self.history.read_tree(tree)?)
    }

    /// The size of `blob` at `path`: the cache's when it holds that blob there, otherwise the
    /// size in the blob's header.
    pub(crate) fn size_of(&self, path: &str, blob: Oid) -> Result<u64, Error> {
        match self.cache.get(path) {
            Some(entry) if entry.state.blob() == Some(blob) => Ok(u64::from(entry.stat.size)),
            Some(_) | None => self.history.blob_len(blob),
        }
    }

    /// The files of checkpoint `id`'s tree, with sizes, sorted by path.
    pub(crate) fn files_of(&self, id: CheckpointId) -> Result<Vec<SizedFile>, Error> {
        self.tree_files(id)?
            .into_iter()
            .map(|file| {
                let size = self.size_of(file.path.as_str(), file.blob)?;
                Ok(SizedFile {
                    path: file.path,
                    blob: file.blob,
                    size,
                })
            })
            .collect()
    }

    /// The changes from checkpoint `before` to `after` (either may be the later), sorted by
    /// path, unflagged: the diff of only the files that differ, read without the subtrees both
    /// share. Sizes are looked up only for the files a change reports. A file a checkpoint
    /// between them read for the first time (`first_read.rs`) is compared from its baseline, so
    /// one unchanged since is no change, and is never a rename's new name, as each checkpoint's
    /// own diff had it.
    pub(crate) fn changes_between(
        &self,
        before: CheckpointId,
        after: CheckpointId,
    ) -> Result<Vec<FileChange>, Error> {
        let (old_tree, new_tree) = (self.timeline.tree(before)?, self.timeline.tree(after)?);
        let (old, new) = self.history.changed_files(old_tree, new_tree)?;
        let (mut old, mut new) = (version_entries(old)?, version_entries(new)?);
        let reads = self.timeline.step_reads(before, after);
        if self.timeline.index(before) <= self.timeline.index(after) {
            baselines::pin(&self.history, &reads.baseline, new_tree, &mut old, &mut new)?;
        } else {
            baselines::pin(&self.history, &reads.baseline, old_tree, &mut new, &mut old)?;
        }
        let there: BTreeSet<&str> = reads.there.keys().map(MatterPath::as_str).collect();
        diff_by(&old, &new, &BTreeSet::new(), &there, &mut |file| {
            self.size_of(file.path.as_str(), file.blob)
        })
    }

    /// The files `walked` (a walk not committed) reads for the first time unchanged, each with
    /// its blob now: the baselines its checkpoint would record.
    pub(crate) fn live_baselines(&self, walked: &Walked) -> Vec<(MatterPath, Oid)> {
        let (record, _) = self.tip_record(walked.cap);
        let versioned = |path: &str| {
            self.cache
                .get(path)
                .is_some_and(|entry| entry.state.blob().is_some())
        };
        let reads = first_reads(&record, &walked.entries, &versioned);
        reads
            .baseline
            .into_iter()
            .filter_map(|file| {
                let entry = find_by_path(&walked.entries, file.path.as_str(), |e| e.path.as_str())?;
                Some((file.path, entry.state.blob()?))
            })
            .collect()
    }

    /// The skip record of the tip commit and whether it was recorded: one written before
    /// records were, or whose blob does not read, gives what the stat cache knows instead.
    fn tip_record(&self, cap: u64) -> (SkipRecord, bool) {
        match skip_record::read(&self.history, self.timeline.tip_skips()) {
            Some(record) => (record, true),
            None => (SkipRecord::of_cache(&self.cache.entries, cap), false),
        }
    }

    /// The skip record of checkpoint `id`: what its walk found in the folder but did not
    /// version. `None` when that is unknown: a commit written before records were, or one
    /// whose record does not read.
    pub(crate) fn skip_record(&self, id: CheckpointId) -> Option<SkipRecord> {
        skip_record::read(&self.history, self.timeline.skips(id))
    }

    /// What the step from `before` to `after` read for the first time.
    pub(crate) fn step_reads(&self, before: CheckpointId, after: CheckpointId) -> StepReads {
        self.timeline.step_reads(before, after)
    }
}

/// `files` (sorted by path bytes) as checked paths.
fn version_entries(files: Vec<TreeFile>) -> Result<Vec<VersionEntry>, Error> {
    files
        .into_iter()
        .map(|file| {
            let path = MatterPath::new(&file.path).map_err(|err| Error::history("tree", err))?;
            Ok(VersionEntry {
                path,
                blob: file.blob,
            })
        })
        .collect()
}

/// One file of a checkpoint's tree, without its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VersionEntry {
    pub(crate) path: MatterPath,
    pub(crate) blob: Oid,
}

impl DiffFile for VersionEntry {
    fn path(&self) -> &MatterPath {
        &self.path
    }

    fn blob(&self) -> Oid {
        self.blob
    }
}

/// The changes from `old` to `new` (trees with sizes), with the files read for the first time
/// `reads`: a baseline is no change (left out of both sides), and a changed one is never taken
/// for the new name of a renamed file.
fn diff_reads(
    old: &[SizedFile],
    new: &[SizedFile],
    over_cap: &BTreeSet<&str>,
    reads: &FirstReads,
) -> Result<Vec<FileChange>, Error> {
    let mut sized = |file: &SizedFile| Ok(file.size);
    if reads.is_empty() {
        return diff_by(old, new, over_cap, &BTreeSet::new(), &mut sized);
    }
    let baseline: BTreeSet<&str> = reads
        .baseline
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    let changed: BTreeSet<&str> = reads
        .changed
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    let without = |files: &[SizedFile]| -> Vec<SizedFile> {
        files
            .iter()
            .filter(|file| !baseline.contains(file.path.as_str()))
            .cloned()
            .collect()
    };
    diff_by(&without(old), &without(new), over_cap, &changed, &mut sized)
}

/// The tree a list of cache entries describes, with sizes.
fn tree_of(entries: &[CacheEntry]) -> Vec<SizedFile> {
    entries
        .iter()
        .filter_map(|entry| {
            entry.state.blob().map(|blob| SizedFile {
                path: entry.path.clone(),
                blob,
                size: u64::from(entry.stat.size),
            })
        })
        .collect()
}

mod baselines;
mod timeline;
mod walk_folder;

#[cfg(test)]
mod tests;
