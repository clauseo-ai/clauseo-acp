//! Trees: encoded bottom-up from a sorted file list in git's entry order, and flattened back.

use std::cmp::Ordering;

use gix::diff::tree::Recorder;
use gix::diff::tree::recorder::Change;
use gix::hash::{Kind as HashKind, ObjectId};
use gix::objs::tree::{EntryKind, EntryMode};
use gix::objs::{Kind, TreeRefIter};
use gix::odb::loose;

use super::check;
use super::durable::{NewObjects, put};
use super::gix_store::{to_gix, to_oid};
use super::{Oid, TreeFile};
use crate::Error;

/// The order of entries in a git tree: by name, a folder's name read as if it ended in `/`.
fn git_order(a: (&[u8], bool), b: (&[u8], bool)) -> Ordering {
    let a_name = a.0.iter().copied().chain(a.1.then_some(b'/'));
    let b_name = b.0.iter().copied().chain(b.1.then_some(b'/'));
    a_name.cmp(b_name)
}

/// Writes through `store` the tree of `files` (sorted by path bytes), whose paths all start
/// with the first `skip` bytes (the tree's own path and its `/`), and the trees under it,
/// skipping trees that exist; records new files in `new_objects`.
pub(super) fn write(
    store: &loose::Store,
    new_objects: &mut NewObjects,
    files: &[TreeFile],
    skip: usize,
) -> Result<ObjectId, Error> {
    let malformed = |path: &str| Error::history("write tree", format!("bad path {path:?}"));
    let mut entries: Vec<(&[u8], bool, ObjectId)> = Vec::new();
    let mut file_names: Vec<&[u8]> = Vec::new();
    let mut rest = files;
    while let Some(first) = rest.first() {
        let path = first.path.as_bytes();
        let tail = path.get(skip..).ok_or_else(|| malformed(&first.path))?;
        let (name, is_folder, id, taken) = match tail.iter().position(|&byte| byte == b'/') {
            None => {
                file_names.push(tail);
                (tail, false, to_gix(first.blob), 1)
            }
            Some(slash) => {
                let name = tail.get(..slash).unwrap_or_default();
                let prefix = path.get(..skip + slash + 1).unwrap_or_default();
                let taken = rest
                    .iter()
                    .take_while(|file| file.path.as_bytes().starts_with(prefix))
                    .count();
                let inside = rest.get(..taken).unwrap_or_default();
                let id = write(store, new_objects, inside, prefix.len())?;
                (name, true, id, taken)
            }
        };
        if name.is_empty() || name.contains(&0) {
            return Err(malformed(&first.path));
        }
        entries.push((name, is_folder, id));
        rest = rest.get(taken..).unwrap_or_default();
    }
    let clash = entries
        .iter()
        .any(|&(name, is_folder, _)| is_folder && file_names.binary_search(&name).is_ok());
    if clash {
        return Err(Error::history(
            "write tree",
            "a path is both a file and a folder",
        ));
    }
    entries.sort_by(|a, b| git_order((a.0, a.1), (b.0, b.1)));
    let mut data = Vec::with_capacity(entries.len() * 48);
    for (name, is_folder, id) in &entries {
        let kind = if *is_folder {
            EntryKind::Tree
        } else {
            EntryKind::Blob
        };
        data.extend_from_slice(kind.as_octal_str());
        data.push(b' ');
        data.extend_from_slice(name);
        data.push(0);
        data.extend_from_slice(id.as_bytes());
    }
    let id = gix::objs::compute_hash(HashKind::Sha1, Kind::Tree, &data)
        .map_err(|err| Error::history("hash", err))?;
    put(store, new_objects, Kind::Tree, &data, id)?;
    Ok(id)
}

/// Appends the files of tree `id` in `store`, whose path is `prefix` (empty or ending in
/// `/`), to `files`; an error for an entry that is neither a blob nor a tree.
pub(super) fn read(
    store: &loose::Store,
    id: ObjectId,
    prefix: &str,
    files: &mut Vec<TreeFile>,
) -> Result<(), Error> {
    let mut buffer = Vec::new();
    let folder = prefix.strip_suffix('/').unwrap_or(prefix);
    let data = find(store, &id, Kind::Tree, folder, &mut buffer)?;
    let mut folders = Vec::new();
    for entry in TreeRefIter::from_bytes(data, HashKind::Sha1) {
        let entry = entry.map_err(|err| Error::history("read tree", err))?;
        let name =
            std::str::from_utf8(entry.filename).map_err(|err| Error::history("read tree", err))?;
        let path = format!("{prefix}{name}");
        if entry.mode.is_tree() {
            folders.push((path, entry.oid.to_owned()));
        } else if entry.mode.is_blob() {
            files.push(TreeFile {
                path,
                blob: to_oid(entry.oid)?,
            });
        } else {
            return Err(Error::history("read tree", format!("{path} is not a file")));
        }
    }
    for (path, id) in folders {
        read(store, id, &format!("{path}/"), files)?;
    }
    Ok(())
}

/// The blob of the file at `path` (`/`-separated) in tree `id`, reading only the trees of the
/// folders on the way; `None` when no file is there (a folder, or nothing).
pub(super) fn file_at(
    store: &loose::Store,
    id: ObjectId,
    path: &str,
) -> Result<Option<Oid>, Error> {
    let mut buffer = Vec::new();
    let mut tree = id;
    let mut parts = path.split('/').peekable();
    let mut folder: usize = 0;
    while let Some(part) = parts.next() {
        let at = path.get(..folder.saturating_sub(1)).unwrap_or_default();
        let data = find(store, &tree, Kind::Tree, at, &mut buffer)?;
        let mut found = None;
        for entry in TreeRefIter::from_bytes(data, HashKind::Sha1) {
            let entry = entry.map_err(|err| Error::history("read tree", err))?;
            if entry.filename == part.as_bytes() {
                found = Some((entry.mode, entry.oid.to_owned()));
                break;
            }
        }
        let Some((mode, oid)) = found else {
            return Ok(None);
        };
        match (parts.peek().is_some(), mode.is_tree()) {
            (true, true) => {
                tree = oid;
                folder += part.len() + 1;
            }
            (false, false) if mode.is_blob() => return to_oid(&oid).map(Some),
            _ => return Ok(None),
        }
    }
    Ok(None)
}

/// Lets `gix::diff::tree` read trees from the loose store, each checked against its id.
struct LooseFind<'s>(&'s loose::Store);

impl gix::objs::Find for LooseFind<'_> {
    fn try_find<'b>(
        &self,
        id: &gix::oid,
        buffer: &'b mut Vec<u8>,
    ) -> gix::Result<Option<gix::objs::Data<'b>>> {
        let data = self.0.try_find(id, buffer)?;
        if let Some(data) = &data {
            data.verify_checksum(id)?;
        }
        Ok(data)
    }
}

/// Appends to `old` and `new` the files of trees `a` and `b` that the other tree does not
/// hold with the same blob, reading no subtree whose id both share (`gix::diff::tree`, git's
/// tree-to-tree diff). Unsorted: git's order is not path order. An error for an entry that is
/// neither a blob nor a tree.
pub(super) fn changed(
    store: &loose::Store,
    a: ObjectId,
    b: ObjectId,
    old: &mut Vec<TreeFile>,
    new: &mut Vec<TreeFile>,
) -> Result<(), Error> {
    if a == b {
        return Ok(());
    }
    let (mut a_buffer, mut b_buffer) = (Vec::new(), Vec::new());
    let a_tree = find(store, &a, Kind::Tree, "", &mut a_buffer)?;
    let b_tree = find(store, &b, Kind::Tree, "", &mut b_buffer)?;
    let mut recorder = Recorder::default();
    gix::diff::tree(
        TreeRefIter::from_bytes(a_tree, HashKind::Sha1),
        TreeRefIter::from_bytes(b_tree, HashKind::Sha1),
        gix::diff::tree::State::default(),
        LooseFind(store),
        &mut recorder,
    )
    .map_err(|err| Error::history("diff trees", err))?;
    for change in recorder.records {
        match change {
            Change::Addition {
                entry_mode,
                oid,
                path,
                ..
            } => push(new, entry_mode, &oid, &path)?,
            Change::Deletion {
                entry_mode,
                oid,
                path,
                ..
            } => push(old, entry_mode, &oid, &path)?,
            Change::Modification {
                previous_entry_mode,
                previous_oid,
                entry_mode,
                oid,
                path,
            } => {
                push(old, previous_entry_mode, &previous_oid, &path)?;
                push(new, entry_mode, &oid, &path)?;
            }
        }
    }
    Ok(())
}

/// Appends the file at `path` to `files` if `mode` is a blob's; a tree's adds nothing (the
/// diff reports its files one by one).
fn push(
    files: &mut Vec<TreeFile>,
    mode: EntryMode,
    oid: &ObjectId,
    path: &[u8],
) -> Result<(), Error> {
    let path = std::str::from_utf8(path).map_err(|err| Error::history("diff trees", err))?;
    if mode.is_tree() {
        return Ok(());
    }
    if !mode.is_blob() {
        return Err(Error::history(
            "diff trees",
            format!("{path} is not a file"),
        ));
    }
    files.push(TreeFile {
        path: path.to_owned(),
        blob: to_oid(oid)?,
    });
    Ok(())
}

/// The content of object `id` in `store`, which must be of `kind` and hash to `id`, read into
/// `buffer`; `path` is the folder it was read for (empty for the root or a commit), named in
/// the error. gix's `try_find` checks the header and the length but not the hash.
pub(super) fn find<'a>(
    store: &loose::Store,
    id: &ObjectId,
    kind: Kind,
    path: &str,
    buffer: &'a mut Vec<u8>,
) -> Result<&'a [u8], Error> {
    let data = store
        .try_find(id, buffer)
        .map_err(|err| Error::history("read object", err))?
        .ok_or_else(|| Error::history("read object", format!("{id} is missing")))?;
    if data.kind != kind {
        return Err(Error::history(
            "read object",
            format!("{id} is not a {kind}"),
        ));
    }
    let found = gix::objs::compute_hash(HashKind::Sha1, kind, data.data)
        .map_err(|err| Error::history("hash", err))?;
    check::same(id, found, "read object", path)?;
    Ok(data.data)
}
