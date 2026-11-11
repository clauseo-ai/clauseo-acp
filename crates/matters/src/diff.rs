//! The changes between two trees: added, modified, deleted and renamed files, and the
//! lawyer-edits flag after a turn.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_protocol::{ChangeKind, ContentId, Count, FileChange, MatterPath, Timestamp};

use crate::Error;
use crate::checkpoint::SizedFile;
use crate::first_read::FirstReads;
use crate::history::Oid;
use crate::skip_record::SkipRecord;
use crate::stat::{CacheEntry, find_by_path};

/// The content id `ContentId` carries for `blob`.
pub(crate) fn content_id(blob: Oid) -> Result<ContentId, Error> {
    ContentId::new(&blob.to_hex()).map_err(|err| Error::history("content id", err))
}

/// One file of a tree as [`diff_by`] sees it.
pub(crate) trait DiffFile {
    /// Its path.
    fn path(&self) -> &MatterPath;
    /// Its blob.
    fn blob(&self) -> Oid;
}

impl DiffFile for SizedFile {
    fn path(&self) -> &MatterPath {
        &self.path
    }

    fn blob(&self) -> Oid {
        self.blob
    }
}

/// The changes from `old` to `new` (both sorted by path), sorted by path. A path that left the
/// tree because it is now over the cap (in `over_cap`) is not a deletion. A deletion and an
/// addition with the same blob are a rename; several are paired in path order.
#[cfg(test)]
pub(crate) fn diff(
    old: &[SizedFile],
    new: &[SizedFile],
    over_cap: &BTreeSet<&str>,
) -> Result<Vec<FileChange>, Error> {
    diff_by(old, new, over_cap, &BTreeSet::new(), &mut |file| {
        Ok(file.size)
    })
}

/// [`diff`] over any files, asking `size` for the size of only the files a change reports.
/// An addition at a path of `not_renamed` (a file that was in the folder before, never
/// versioned: `first_read.rs`) is never paired as a rename's new name.
pub(crate) fn diff_by<F: DiffFile>(
    old: &[F],
    new: &[F],
    over_cap: &BTreeSet<&str>,
    not_renamed: &BTreeSet<&str>,
    size_of: &mut dyn FnMut(&F) -> Result<u64, Error>,
) -> Result<Vec<FileChange>, Error> {
    let mut added: Vec<&F> = Vec::new();
    let mut deleted: Vec<&F> = Vec::new();
    let mut modified: Vec<(&F, &F)> = Vec::new();
    let (mut old_iter, mut new_iter) = (old.iter().peekable(), new.iter().peekable());
    loop {
        match (old_iter.peek(), new_iter.peek()) {
            (None, None) => break,
            (Some(o), Some(n)) if o.path().as_str() == n.path().as_str() => {
                if o.blob() != n.blob() {
                    modified.push((o, n));
                }
                old_iter.next();
                new_iter.next();
            }
            (Some(o), Some(n)) if o.path().as_str().as_bytes() < n.path().as_str().as_bytes() => {
                deleted.push(o);
                old_iter.next();
            }
            (Some(_) | None, Some(n)) => {
                added.push(n);
                new_iter.next();
            }
            (Some(o), None) => {
                deleted.push(o);
                old_iter.next();
            }
        }
    }
    deleted.retain(|file| !over_cap.contains(file.path().as_str()));
    // Pair renames: each added file takes the first unused deleted file with its blob.
    let mut by_blob: BTreeMap<Oid, Vec<&F>> = BTreeMap::new();
    for file in &deleted {
        by_blob.entry(file.blob()).or_default().push(file);
    }
    let mut changes = Vec::with_capacity(added.len() + deleted.len() + modified.len());
    let mut renamed_from: BTreeSet<&str> = BTreeSet::new();
    for file in added {
        let from = by_blob
            .get_mut(&file.blob())
            .filter(|_| !not_renamed.contains(file.path().as_str()))
            .and_then(|candidates| (!candidates.is_empty()).then(|| candidates.remove(0)));
        let blob = content_id(file.blob())?;
        let bytes = size(size_of(file)?);
        let change = match from {
            Some(from) => {
                renamed_from.insert(from.path().as_str());
                let kind = ChangeKind::Renamed {
                    from: from.path().clone(),
                };
                FileChange::new(
                    file.path().clone(),
                    kind,
                    Some(blob.clone()),
                    Some(blob),
                    bytes,
                )
            }
            None => FileChange::new(
                file.path().clone(),
                ChangeKind::Added,
                None,
                Some(blob),
                bytes,
            ),
        };
        changes.push(change.map_err(|err| Error::history("diff", err))?);
    }
    for file in deleted {
        if renamed_from.contains(file.path().as_str()) {
            continue;
        }
        let blob = content_id(file.blob())?;
        let change = FileChange::new(
            file.path().clone(),
            ChangeKind::Deleted,
            Some(blob),
            None,
            size(size_of(file)?),
        );
        changes.push(change.map_err(|err| Error::history("diff", err))?);
    }
    for (o, n) in modified {
        let change = FileChange::new(
            n.path().clone(),
            ChangeKind::Modified,
            Some(content_id(o.blob())?),
            Some(content_id(n.blob())?),
            size(size_of(n)?),
        );
        changes.push(change.map_err(|err| Error::history("diff", err))?);
    }
    changes.sort_by(|a, b| {
        a.path()
            .as_str()
            .as_bytes()
            .cmp(b.path().as_str().as_bytes())
    });
    Ok(changes)
}

fn size(bytes: u64) -> Count {
    Count::clamped(bytes)
}

/// What an after-turn checkpoint knew of the folder at the start of its step (the last commit),
/// what its walk found and what the turn's shell commands named, to tell a file the agent made
/// though no tool call named it (a document Bash wrote with pandoc) from one the lawyer may have
/// saved meanwhile: see [`TurnStart::own`].
pub(crate) struct TurnStart<'a> {
    /// The files the turn's shell commands wrote, as paths in the matter.
    pub(crate) commands: &'a BTreeSet<String>,
    /// The last commit's skip record; `None` when it has none (written before records were).
    pub(crate) record: Option<&'a SkipRecord>,
    /// The last commit's tree.
    pub(crate) tree: &'a [SizedFile],
    /// What this checkpoint reads for the first time.
    pub(crate) reads: &'a FirstReads,
    /// The walk's entries, sorted by path bytes.
    pub(crate) entries: &'a [CacheEntry],
    /// The checkpoint's time: the turn's end.
    pub(crate) at: Timestamp,
}

impl TurnStart<'_> {
    /// Whether `change` adds a file a shell command of the turn wrote (its whole path), that
    /// the step's start record proves absent (as undo needs it to delete the file:
    /// [`SkipRecord::not_absent`]), not read for the first time, and last modified at or
    /// before the checkpoint's time: the agent's, though no tool call named it. A file named by
    /// nothing may be one the lawyer saved during the turn.
    fn own(&self, change: &FileChange) -> bool {
        let path = change.path();
        let (ChangeKind::Added, Some(record)) = (change.change(), self.record) else {
            return false;
        };
        if !self.commands.contains(path.as_str()) {
            return false;
        }
        let in_tree = find_by_path(self.tree, path.as_str(), |file| file.path.as_str()).is_some();
        if record.not_absent(path.as_str(), in_tree).is_some()
            || self.reads.changed.iter().any(|file| file.path == *path)
        {
            return false;
        }
        find_by_path(self.entries, path.as_str(), |entry| entry.path.as_str()).is_some_and(
            |entry| {
                let mtime = i64::from(entry.stat.mtime.secs) * 1000
                    + i64::from(entry.stat.mtime.nanos / 1_000_000);
                mtime <= self.at.as_millis()
            },
        )
    }
}

/// `change`, flagged as possibly holding the lawyer's edits when no tool call named its path
/// (for a rename: neither path) and it is not a file the agent added that a shell command wrote
/// (`start`: [`TurnStart::own`]).
pub(crate) fn flag(
    change: FileChange,
    tools: &BTreeSet<MatterPath>,
    start: &TurnStart<'_>,
) -> FileChange {
    let named = tools.contains(change.path())
        || matches!(change.change(), ChangeKind::Renamed { from } if tools.contains(from));
    if named || start.own(&change) {
        change
    } else {
        change.with_lawyer_edits()
    }
}

#[cfg(test)]
mod tests;
