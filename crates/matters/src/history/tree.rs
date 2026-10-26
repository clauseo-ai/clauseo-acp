//! Trees: encoded bottom-up from a sorted file list in git's entry order, and flattened back.

use std::cmp::Ordering;

use gix::hash::{Kind as HashKind, ObjectId};
use gix::objs::tree::EntryKind;
use gix::objs::{Kind, TreeRefIter};
use gix::odb::loose;

use super::TreeFile;
use super::durable::{NewObjects, put};
use super::gix_store::{to_gix, to_oid};
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
    let data = find(store, &id, Kind::Tree, &mut buffer)?;
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

/// The content of object `id` in `store`, which must be of `kind`, read into `buffer`.
pub(super) fn find<'a>(
    store: &loose::Store,
    id: &ObjectId,
    kind: Kind,
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
    Ok(data.data)
}
