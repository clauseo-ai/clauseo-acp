//! The durable half of a checkpoint: the object files written since the last publish and the
//! syncs that make them durable, and the timeline ref, read and moved without gix's ref store.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use gix::hash::ObjectId;
use gix::objs::{Kind, Write as _};
use gix::odb::loose;

use super::gix_store::{sync_dir, to_oid};
use super::{Oid, TIMELINE_REF};
use crate::{Error, fsync, write};

/// The loose object files written since the last publish, and their `objects/xx` folders.
#[derive(Debug, Default)]
pub(super) struct NewObjects {
    /// Each new object file, in the order written.
    files: Vec<PathBuf>,
    /// The `objects/xx` folders those files are in.
    folders: BTreeSet<PathBuf>,
    /// Whether any of those folders was new, so `objects` gained an entry.
    made_folder: bool,
}

impl NewObjects {
    /// Records the new object file `file`; `made_folder` when its folder was made for it.
    pub(super) fn record(&mut self, file: PathBuf, made_folder: bool) {
        if let Some(folder) = file.parent() {
            self.folders.insert(folder.to_path_buf());
        }
        self.made_folder |= made_folder;
        self.files.push(file);
    }

    /// The files recorded so far.
    #[cfg(test)]
    pub(super) fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// Steps 1 and 2 of the sync order: syncs each new file, each folder they are in, and
    /// `objects` (the folder `objects`) if a folder was new; then one barrier on a new file.
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

/// Whether object `id` is in `store` with a sound header: its file exists and its first bytes
/// inflate to a header of `kind` and `size`. A file that does not (torn by a crash before it
/// was synced) is logged and removed, so the caller writes the object again.
pub(super) fn present(
    store: &loose::Store,
    id: &ObjectId,
    kind: Kind,
    size: u64,
) -> Result<bool, Error> {
    let path = store.object_path(id);
    if !path.is_file() {
        return Ok(false);
    }
    match store.try_header(id) {
        Ok(Some(header)) if header == (size, kind) => return Ok(true),
        Ok(header) => {
            tracing::warn!(path = %path.display(), ?header, "rewriting a torn loose object");
        }
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "rewriting a torn loose object");
        }
    }
    match write::remove_file(&path) {
        Ok(()) => Ok(false),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(Error::io("remove", path, err)),
    }
}

/// Writes object `id` (`kind`, `data`) through `store` unless it is [`present`], and records
/// the new file in `new_objects`.
pub(super) fn put(
    store: &loose::Store,
    new_objects: &mut NewObjects,
    kind: Kind,
    data: &[u8],
    id: ObjectId,
) -> Result<(), Error> {
    let size = u64::try_from(data.len()).unwrap_or(u64::MAX);
    if present(store, &id, kind, size)? {
        return Ok(());
    }
    let path = store.object_path(&id);
    let made_folder = !path.parent().is_some_and(Path::is_dir);
    store
        .write_buf_with_known_id(kind, data, id)
        .map_err(|err| Error::history("write object", err))?;
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
