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

/// One entry of a tree: its name, whether it is a folder, and its id.
type Entry = (Vec<u8>, bool, ObjectId);

/// The entries of tree `id` in `store`, in git's order; an error for an entry that is neither
/// a blob nor a tree.
fn entries(store: &loose::Store, id: &ObjectId) -> Result<Vec<Entry>, Error> {
    let mut buffer = Vec::new();
    let data = find(store, id, Kind::Tree, &mut buffer)?;
    let mut entries = Vec::new();
    for entry in TreeRefIter::from_bytes(data, HashKind::Sha1) {
        let entry = entry.map_err(|err| Error::history("read tree", err))?;
        let is_folder = entry.mode.is_tree();
        if !is_folder && !entry.mode.is_blob() {
            let name = String::from_utf8_lossy(entry.filename);
            return Err(Error::history("read tree", format!("{name} is not a file")));
        }
        entries.push((entry.filename.to_vec(), is_folder, entry.oid.to_owned()));
    }
    Ok(entries)
}

/// Appends to `old` and `new` the files of trees `a` and `b` (whose path is `prefix`, empty
/// or ending in `/`) that the other tree does not hold with the same blob, skipping every
/// subtree whose id is the same on both sides. Unsorted: git's order is not path order.
pub(super) fn changed(
    store: &loose::Store,
    a: ObjectId,
    b: ObjectId,
    prefix: &str,
    old: &mut Vec<TreeFile>,
    new: &mut Vec<TreeFile>,
) -> Result<(), Error> {
    if a == b {
        return Ok(());
    }
    let (a_entries, b_entries) = (entries(store, &a)?, entries(store, &b)?);
    let (mut a_iter, mut b_iter) = (
        a_entries.into_iter().peekable(),
        b_entries.into_iter().peekable(),
    );
    loop {
        let order = match (a_iter.peek(), b_iter.peek()) {
            (None, None) => break,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(x), Some(y)) => git_order((x.0.as_slice(), x.1), (y.0.as_slice(), y.1)),
        };
        match order {
            Ordering::Less => {
                if let Some(entry) = a_iter.next() {
                    take(store, &entry, prefix, old)?;
                }
            }
            Ordering::Greater => {
                if let Some(entry) = b_iter.next() {
                    take(store, &entry, prefix, new)?;
                }
            }
            Ordering::Equal => {
                let (Some(x), Some(y)) = (a_iter.next(), b_iter.next()) else {
                    break;
                };
                if x.2 == y.2 {
                    continue;
                }
                if x.1 {
                    let path = format!("{prefix}{}/", name_of(&x.0)?);
                    changed(store, x.2, y.2, &path, old, new)?;
                } else {
                    take(store, &x, prefix, old)?;
                    take(store, &y, prefix, new)?;
                }
            }
        }
    }
    Ok(())
}

fn name_of(name: &[u8]) -> Result<&str, Error> {
    std::str::from_utf8(name).map_err(|err| Error::history("read tree", err))
}

/// Appends the file `entry` under `prefix` to `files`, or every file under it if a folder.
fn take(
    store: &loose::Store,
    entry: &Entry,
    prefix: &str,
    files: &mut Vec<TreeFile>,
) -> Result<(), Error> {
    let path = format!("{prefix}{}", name_of(&entry.0)?);
    if entry.1 {
        read(store, entry.2, &format!("{path}/"), files)
    } else {
        files.push(TreeFile {
            path,
            blob: to_oid(&entry.2)?,
        });
        Ok(())
    }
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
