//! The durable half of a checkpoint: the object files written since the last publish and the
//! syncs that make them durable, and the timeline ref, read and moved without gix's ref store.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::objects::Writer;
use gix::hash::ObjectId;
use gix::objs::Kind;

use super::check;
use super::gix_store::{sync_dir, to_oid};
use super::{Oid, TIMELINE_REF};
use crate::{Error, fsync, write};

/// The loose object files a commit about to be published may newly name: those written since
/// the last publish and those found already present, with their `objects/xx` folders.
#[derive(Debug, Default)]
pub(super) struct NewObjects {
    /// Each object file to sync.
    files: BTreeSet<PathBuf>,
    /// The `objects/xx` folders those files are in.
    folders: BTreeSet<PathBuf>,
    /// Whether any of those folders was new, so `objects` gained an entry.
    made_folder: bool,
}

impl NewObjects {
    /// Records the object file `file`; `made_folder` when its folder was made for it.
    pub(super) fn record(&mut self, file: PathBuf, made_folder: bool) {
        if let Some(folder) = file.parent() {
            self.folders.insert(folder.to_path_buf());
        }
        self.made_folder |= made_folder;
        self.files.insert(file);
    }

    /// The files recorded so far.
    #[cfg(test)]
    pub(super) fn files(&self) -> Vec<PathBuf> {
        self.files.iter().cloned().collect()
    }

    /// Steps 1 and 2 of the sync order: syncs each recorded file, each folder they are in,
    /// and `objects` (the folder `objects`) if a folder was new; then one barrier on a file.
    pub(super) fn make_durable(&self, objects: &Path) -> Result<(), Error> {
        let mut last = None;
        for path in &self.files {
            let file = write::open_to_sync(path).map_err(|err| Error::io("open", path, err))?;
            fsync::sync_file(&file).map_err(|err| Error::io("sync", path, err))?;
            last = Some((file, path));
        }
        for folder in &self.folders {
            sync_dir(folder)?;
        }
        if self.made_folder {
            sync_dir(objects)?;
        }
        if let Some((file, path)) = last {
            fsync::barrier(&file).map_err(|err| Error::io("sync", path, err))?;
        }
        Ok(())
    }

    /// Forgets every recorded file, once they are durable and named.
    pub(super) fn clear(&mut self) {
        self.files.clear();
        self.folders.clear();
        self.made_folder = false;
    }
}

/// The content of an object to store, as [`present`] checks a copy already stored.
#[derive(Clone, Copy)]
pub(super) enum Content<'a> {
    /// Its bytes, held in memory (trees, commits, files read whole).
    InHand(&'a [u8]),
    /// Only its size: a streamed file, whose bytes are not held.
    Streamed(u64),
}

/// Whether object `id` (`kind`, `content`) is in `store` and sound; if so it is recorded in
/// `new_objects` for the sync. Every file found is inflated in full, whenever it was written:
/// one from before the last publish (a revert to an old version, say) is checked as closely as
/// one a crashed run left, so a damaged copy is written again while the checkpoint has the
/// right bytes, not found at undo or restore. Bytes in hand are compared with what it inflates
/// to ([`check::same`]); a streamed file's object is hashed against `id` ([`check::whole`]).
/// A file that fails is logged and left for the caller to write over: gix renames the new
/// object onto it (on Windows, where that rename is skipped when the target exists, it is
/// removed first). An I/O error reading it is returned and the file kept.
pub(super) fn present(
    store: Writer<'_>,
    new_objects: &mut NewObjects,
    id: &ObjectId,
    kind: Kind,
    content: Content<'_>,
) -> Result<bool, Error> {
    let path = store.object_path(id);
    if new_objects.files.contains(&path) {
        return Ok(true);
    }
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(Error::io("stat", path, err)),
    };
    let sound = metadata.is_file()
        && match content {
            Content::InHand(data) => check::same(&path, kind, data)?,
            Content::Streamed(size) => check::whole(&path, id, kind, size)?,
        };
    if sound {
        new_objects.record(path, false);
        return Ok(true);
    }
    tracing::warn!("rewriting a torn loose object");
    #[cfg(windows)]
    match write::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(Error::io("remove", path, err)),
    }
    Ok(false)
}

/// Writes object `id` (`kind`, `data`) through `store` unless it is [`present`], and records
/// the file in `new_objects`.
pub(super) fn put(
    store: Writer<'_>,
    new_objects: &mut NewObjects,
    kind: Kind,
    data: &[u8],
    id: ObjectId,
) -> Result<(), Error> {
    if present(store, new_objects, &id, kind, Content::InHand(data))? {
        return Ok(());
    }
    let path = store.object_path(&id);
    let made_folder = !path.parent().is_some_and(Path::is_dir);
    store.write_buf(kind, data, id)?;
    new_objects.record(path, made_folder);
    Ok(())
}

/// The timeline ref's file in the git dir `git_dir`.
fn ref_path(git_dir: &Path) -> PathBuf {
    git_dir.join(TIMELINE_REF)
}

/// The commit the timeline ref names, or `None` when its file does not exist.
pub(super) fn read_ref(git_dir: &Path) -> Result<Option<Oid>, Error> {
    let path = ref_path(git_dir);
    let text = match std::fs::read(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::io("read", path, err)),
    };
    let id = gix::ObjectId::from_hex(text.trim_ascii())
        .map_err(|err| Error::history("read timeline", err))?;
    to_oid(&id).map(Some)
}

/// An error unless the timeline ref names `expected` (`None`: it does not exist).
pub(super) fn check_ref(git_dir: &Path, expected: Option<Oid>) -> Result<(), Error> {
    let current = read_ref(git_dir)?;
    if current == expected {
        return Ok(());
    }
    let message = format!("the timeline is at {current:?}, expected {expected:?}");
    Err(Error::history("move timeline", message))
}

/// Step 3 of the sync order: moves the timeline ref from `expected` to `commit` (an error
/// that moves nothing if it is elsewhere) by a synced temp file renamed over the ref, then
/// syncs its folder, and `refs` when that folder was new.
pub(super) fn move_ref(git_dir: &Path, commit: Oid, expected: Option<Oid>) -> Result<(), Error> {
    check_ref(git_dir, expected)?;
    let path = ref_path(git_dir);
    let folder = path
        .parent()
        .ok_or_else(|| Error::io("create", &path, io::ErrorKind::InvalidInput.into()))?;
    let made_folder = match write::create_dir(folder) {
        Ok(()) => true,
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => false,
        Err(err) => return Err(Error::io("create", folder, err)),
    };
    let text = format!("{}\n", commit.to_hex());
    write::replace(&path, &mut |file| {
        file.write_all(text.as_bytes())
            .map_err(|err| Error::io("write", &path, err))
    })?;
    if made_folder && let Some(refs) = folder.parent() {
        sync_dir(refs)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[cfg(windows)]
mod windows_tests;
