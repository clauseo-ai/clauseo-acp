//! Taking a checkpoint: walk the folder against the stat cache, hash only what changed, build
//! the tree, diff it against the last checkpoint, and commit it unless nothing changed.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use clauseo_protocol::{
    CheckpointId, CheckpointKind, FileChange, MatterPath, SkippedFile, SkippedFileReason, Timestamp,
};

use crate::diff::{diff, flag};
use crate::history::{CommitRecord, GixStore, HistoryStore, Oid, TreeFile};
use crate::stat::{CacheEntry, EntryState, Stat, StatCache};
use crate::types::{Checkpoint, CheckpointOutcome, Progress, Settings, TimelineEntry};
use crate::walk::{self, FoundKind};
use crate::{Error, trailers};

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

/// The checkpoints, oldest first, and their commits, plus the ref's tip.
#[derive(Debug, Default)]
pub(crate) struct Timeline {
    pub(crate) entries: Vec<TimelineEntry>,
    commits: Vec<(Oid, Oid)>,
    by_id: HashMap<CheckpointId, usize>,
    /// The commit the ref names and its tree, checkpoint or not: the parent of the next commit
    /// and the tree the stat cache describes.
    tip: Option<(Oid, Oid)>,
}

impl Timeline {
    /// The timeline of `records` (oldest first). A commit without a valid `Clauseo-Checkpoint`
    /// trailer is left out with a warning (it can still be the tip); of two commits with one id,
    /// the later wins.
    pub(crate) fn from_records(records: &[CommitRecord]) -> Self {
        let mut timeline = Self::default();
        for record in records {
            if let Some(entry) = trailers::parse(&record.message) {
                timeline.push(entry, record.id, record.tree);
            } else {
                tracing::warn!(commit = ?record.id, "a commit without a checkpoint id");
            }
        }
        timeline.tip = records.last().map(|record| (record.id, record.tree));
        timeline
    }

    fn push(&mut self, entry: TimelineEntry, commit: Oid, tree: Oid) {
        self.by_id.insert(entry.id, self.entries.len());
        self.entries.push(entry);
        self.commits.push((commit, tree));
        self.tip = Some((commit, tree));
    }

    /// The last checkpoint: its id, commit and tree. Not always the tip: a later version may
    /// put a commit without a trailer after it.
    pub(crate) fn head(&self) -> Option<(CheckpointId, Oid, Oid)> {
        let entry = self.entries.last()?;
        let (commit, tree) = self.commits.last()?;
        Some((entry.id, *commit, *tree))
    }

    /// The commit the ref names and its tree.
    pub(crate) const fn tip(&self) -> Option<(Oid, Oid)> {
        self.tip
    }

    /// The tree of checkpoint `id`.
    pub(crate) fn tree(&self, id: CheckpointId) -> Result<Oid, Error> {
        self.by_id
            .get(&id)
            .and_then(|index| self.commits.get(*index))
            .map(|(_, tree)| *tree)
            .ok_or(Error::UnknownCheckpoint { id })
    }
}

/// What a checkpoint is for: its trailers and, after a turn, the paths the agent's tool calls
/// named.
pub(crate) struct Spec<'a> {
    pub(crate) entry: TimelineEntry,
    pub(crate) at: Timestamp,
    /// `Some` for an `after-turn` checkpoint: a change to a path outside the set may hold the
    /// lawyer's edits.
    pub(crate) tool_paths: Option<&'a BTreeSet<MatterPath>>,
}

/// A walk's result: the new cache entries, the skipped files worth listing, and whether any file
/// was read.
pub(crate) struct Walked {
    entries: Vec<CacheEntry>,
    skipped: Vec<SkippedFile>,
    /// A file was read: if only racily clean files were, the entries may equal the cache's and
    /// the cache must still be written, so its time moves past them.
    read_any: bool,
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
    fn check_cache(&mut self) -> Result<(), Error> {
        let Some((_, tree)) = self.timeline.tip() else {
            self.cache = StatCache::default();
            self.cache_tree = None;
            return Ok(());
        };
        if self.cache_tree == Some(tree) {
            return Ok(());
        }
        let files = self.history.read_tree(tree)?;
        let cached = self
            .cache
            .entries
            .iter()
            .filter_map(|entry| entry.state.blob().map(|blob| (entry.path.as_str(), blob)));
        let same = cached.eq(files.iter().map(|file| (file.path.as_str(), file.blob)));
        if !same {
            tracing::info!("the stat cache does not match the timeline's tip; rebuilding it");
            let mut entries = Vec::with_capacity(files.len());
            for file in files {
                let path =
                    MatterPath::new(&file.path).map_err(|err| Error::history("tree", err))?;
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
            self.cache = StatCache {
                entries,
                written_at: None,
            };
        }
        self.cache_tree = Some(tree);
        Ok(())
    }

    /// Walks `folder` and hashes and stores every file the cache cannot vouch for. `Ok(None)`
    /// when cancelled. A file found over the cap when read is skipped like one the walk found
    /// over it; the entries under a subfolder that does not read are kept as they were.
    pub(crate) fn scan_folder(
        &mut self,
        folder: &Path,
        settings: &Settings,
        cancel: Option<&AtomicBool>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Option<Walked>, Error> {
        self.check_cache()?;
        let probe = settings.probe.as_ref();
        let cap = settings.limits.effective_cap();
        let Some(listing) = walk::walk(folder, probe, cap, cancel, &mut |_| {})? else {
            return Ok(None);
        };
        let mut entries = Vec::with_capacity(listing.files.len());
        let mut skipped = Vec::new();
        let mut done = Progress { files: 0, bytes: 0 };
        let mut read_any = false;
        for mut file in listing.files {
            if cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
                return Ok(None);
            }
            let old = self.cache.get(file.path.as_str());
            let mut state = None;
            if file.kind == FoundKind::File {
                if let Some(blob) = self.cache.clean_blob(file.path.as_str(), &file.stat) {
                    state = Some(EntryState::Read(blob));
                } else {
                    let path = folder.join(file.path.as_str());
                    match self.history.store_file(&path, file.len, cap) {
                        Ok((blob, read)) => {
                            done.bytes = done.bytes.saturating_add(read);
                            read_any = true;
                            state = Some(EntryState::Read(blob));
                        }
                        // Gone since the walk listed it: it is deleted.
                        Err(Error::Io { source, .. })
                            if source.kind() == std::io::ErrorKind::NotFound =>
                        {
                            continue;
                        }
                        // Grown over the cap since the walk listed it: skipped, with its stat now.
                        Err(Error::Io { source, .. })
                            if source.kind() == std::io::ErrorKind::FileTooLarge =>
                        {
                            file.kind = FoundKind::OverCap;
                            if let Ok(metadata) = std::fs::symlink_metadata(&path) {
                                file.stat = Stat::from_metadata(&metadata);
                            }
                        }
                        Err(err) => return Err(err),
                    }
                }
            }
            let state = if let Some(state) = state {
                state
            } else {
                let reason = if file.kind == FoundKind::CloudOnly {
                    SkippedFileReason::CloudOnly
                } else {
                    SkippedFileReason::OverSizeCap
                };
                if worth_listing(old, &file.stat) {
                    skipped.push(SkippedFile {
                        path: file.path.clone(),
                        reason,
                    });
                }
                // A versioned file that is now cloud-only keeps its blob: dehydration does
                // not change content. A file over the cap leaves the tree.
                match old.map(|old| old.state.blob()) {
                    Some(Some(blob)) if file.kind == FoundKind::CloudOnly => {
                        EntryState::Unread(blob)
                    }
                    Some(_) | None => EntryState::Skipped,
                }
            };
            done.files += 1;
            progress(done);
            entries.push(CacheEntry {
                path: file.path,
                stat: file.stat,
                state,
            });
        }
        if !listing.unread.is_empty() {
            self.keep_unread(&mut entries, &listing.unread);
        }
        Ok(Some(Walked {
            entries,
            skipped,
            read_any,
        }))
    }

    /// Adds to `entries` (sorted) the cache's entries under each folder of `unread` that the
    /// walk did not find, as they were, and sorts them again.
    fn keep_unread(&self, entries: &mut Vec<CacheEntry>, unread: &[String]) {
        let found: BTreeSet<String> = entries
            .iter()
            .map(|entry| entry.path.as_str().to_owned())
            .collect();
        for dir in unread {
            let prefix = format!("{dir}/");
            let kept = self.cache.entries.iter().filter(|entry| {
                entry.path.as_str().starts_with(&prefix) && !found.contains(entry.path.as_str())
            });
            entries.extend(kept.cloned());
        }
        entries.sort_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
        entries.dedup_by(|a, b| a.path == b.path);
    }

    /// Commits `walked` as checkpoint `spec`, or reuses the last checkpoint when the folder
    /// equals the tip's tree and that is the last checkpoint's tree (never for the root
    /// checkpoint), and writes the cache. The parent is the tip, checkpoint or not.
    pub(crate) fn commit(&mut self, walked: Walked, spec: &Spec<'_>) -> Result<Checkpoint, Error> {
        let Walked {
            entries,
            skipped,
            read_any,
        } = walked;
        let old_tree = tree_of(&self.cache.entries);
        let new_tree = tree_of(&entries);
        let over_cap: BTreeSet<&str> = entries
            .iter()
            .filter(|entry| entry.state == EntryState::Skipped)
            .map(|entry| entry.path.as_str())
            .collect();
        let mut changes = diff(&old_tree, &new_tree, &over_cap)?;
        let head = self.timeline.head();
        let tip = self.timeline.tip();
        let same_tree = old_tree.len() == new_tree.len()
            && old_tree
                .iter()
                .zip(&new_tree)
                .all(|(old, new)| old.path == new.path && old.blob == new.blob);
        let tip_is_head = head.map(|(_, _, tree)| tree) == tip.map(|(_, tree)| tree);
        if let (Some((last, _, _)), true) = (head, same_tree && tip_is_head) {
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
                changes,
                skipped,
            });
        }
        if self.timeline.by_id.contains_key(&spec.entry.id) {
            return Err(Error::DuplicateCheckpoint { id: spec.entry.id });
        }
        if let Some(tools) = spec.tool_paths {
            changes = changes
                .into_iter()
                .map(|change| flag(change, tools))
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
        let message = trailers::message(&spec.entry, count);
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
                tracing::warn!(error = %reload, "the timeline did not read after a failed publish");
            }
            return Err(err);
        }
        self.cache = cache;
        self.cache_tree = Some(tree);
        self.timeline.push(spec.entry, commit, tree);
        Ok(Checkpoint {
            outcome: CheckpointOutcome::Created(spec.entry.id),
            changes,
            skipped,
        })
    }

    /// Walks and commits in one go; never cancelled.
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

    /// The files of checkpoint `id`'s tree, with sizes, sorted by path.
    pub(crate) fn files_of(&self, id: CheckpointId) -> Result<Vec<SizedFile>, Error> {
        let tree = self.timeline.tree(id)?;
        let files = self.history.read_tree(tree)?;
        files
            .into_iter()
            .map(|file| {
                let path =
                    MatterPath::new(&file.path).map_err(|err| Error::history("tree", err))?;
                let size = match self.cache.get(&file.path) {
                    Some(entry) if entry.state.blob() == Some(file.blob) => {
                        u64::from(entry.stat.size)
                    }
                    Some(_) | None => self.history.blob_len(file.blob)?,
                };
                Ok(SizedFile {
                    path,
                    blob: file.blob,
                    size,
                })
            })
            .collect()
    }

    /// The changes from checkpoint `before` to `after`, sorted by path, unflagged.
    pub(crate) fn changes_between(
        &self,
        before: CheckpointId,
        after: CheckpointId,
    ) -> Result<Vec<FileChange>, Error> {
        let old = self.files_of(before)?;
        let new = self.files_of(after)?;
        diff(&old, &new, &BTreeSet::new())
    }
}

/// Whether a skipped file goes in a checkpoint's `skipped` list: it is new, or its size or
/// modification time changed. ctime, device and inode are left out: a sync client touches a
/// placeholder's attributes without changing the file.
pub(crate) fn worth_listing(old: Option<&CacheEntry>, stat: &Stat) -> bool {
    old.is_none_or(|old| old.stat.size != stat.size || old.stat.mtime != stat.mtime)
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

#[cfg(test)]
mod tests;
