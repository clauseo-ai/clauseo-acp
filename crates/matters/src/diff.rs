//! The changes between two trees: added, modified, deleted and renamed files, and the
//! lawyer-edits flag after a turn.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_protocol::{ChangeKind, ContentId, Count, FileChange, MatterPath};

use crate::Error;
use crate::checkpoint::SizedFile;
use crate::history::Oid;

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
pub(crate) fn diff(
    old: &[SizedFile],
    new: &[SizedFile],
    over_cap: &BTreeSet<&str>,
) -> Result<Vec<FileChange>, Error> {
    diff_by(old, new, over_cap, &mut |file| Ok(file.size))
}

/// [`diff`] over any files, asking `size` for the size of only the files a change reports.
pub(crate) fn diff_by<F: DiffFile>(
    old: &[F],
    new: &[F],
    over_cap: &BTreeSet<&str>,
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

/// `change`, flagged as possibly holding the lawyer's edits when no tool call named its path
/// (for a rename: neither path).
pub(crate) fn flag(change: FileChange, tools: &BTreeSet<MatterPath>) -> FileChange {
    let named = tools.contains(change.path())
        || matches!(change.change(), ChangeKind::Renamed { from } if tools.contains(from));
    if named {
        change
    } else {
        change.with_lawyer_edits()
    }
}

#[cfg(test)]
mod tests;
