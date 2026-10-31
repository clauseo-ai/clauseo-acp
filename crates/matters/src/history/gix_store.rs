//! [`GixStore`]: the [`HistoryStore`] over gix's loose object store. Commits are encoded and
//! hashed here (blobs in `blob.rs`, trees in `tree.rs`) and written through two loose stores
//! on one `objects` folder, so the compression level is chosen per object and every new object
//! file is known for the sync in [`HistoryStore::publish`].

use std::fs::File;
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};

use gix::hash::{Kind as HashKind, ObjectId};
use gix::objs::{CommitRef, Kind};
use gix::odb::loose;
use gix::zlib::Compression;

use super::blob::{Blobs, Target, count};
use super::durable::{self, NewObjects, put};
use super::{CommitRecord, HEAD_REF, HistoryStore, Oid, TreeFile, index_file, tree};
use crate::stat::{FileTime, StatCache};
use crate::{Error, fsync, write};

/// The chunk in which a blob is inflated by [`HistoryStore::read_blob`].
const READ_CHUNK: usize = 64 * 1024;

/// The shadow repository of one matter: its git dir and the loose object stores in it.
pub(crate) struct GixStore {
    /// `history.git`.
    git_dir: PathBuf,
    /// Writes blobs that are already compressed, at zlib level 0.
    stored: loose::Store,
    /// Writes every other object at zlib level 1, and reads every object.
    deflated: loose::Store,
    /// Reads files as blobs.
    blobs: Blobs,
    /// Object files written since the last publish.
    new_objects: NewObjects,
}

impl std::fmt::Debug for GixStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GixStore")
            .field("git_dir", &self.git_dir)
            .field("new_objects", &self.new_objects)
            .finish_non_exhaustive()
    }
}

/// The gix id of `oid`.
pub(super) const fn to_gix(oid: Oid) -> ObjectId {
    ObjectId::Sha1(oid.0)
}

/// The [`Oid`] of a gix id; an error for an id that is not SHA-1.
pub(super) fn to_oid(id: &gix::oid) -> Result<Oid, Error> {
    <[u8; 20]>::try_from(id.as_bytes())
        .map(Oid)
        .map_err(|err| Error::history("object id", err))
}

/// `HEAD`: a symbolic ref to [`HEAD_REF`], a branch that is never born.
fn head_text() -> String {
    format!("ref: {HEAD_REF}\n")
}

/// `config`, with `core.worktree` set to `worktree` (absolute and UTF-8, else an error).
fn config_text(worktree: &Path) -> Result<String, Error> {
    let path = worktree.to_str().ok_or_else(|| {
        Error::history(
            "config",
            format!("{} is not valid UTF-8", worktree.display()),
        )
    })?;
    if !worktree.is_absolute() {
        return Err(Error::history("config", format!("{path} is not absolute")));
    }
    let mut quoted = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\t' => quoted.push_str("\\t"),
            other => quoted.push(other),
        }
    }
    Ok(format!(
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = false\n\tbare = false\n\
         \tlogallrefupdates = false\n\tworktree = \"{quoted}\"\n"
    ))
}

/// Creates the new file `path` with `bytes` and syncs it.
fn new_file(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = write::create_new(path).map_err(|err| Error::io("create", path, err))?;
    file.write_all(bytes)
        .map_err(|err| Error::io("write", path, err))?;
    fsync::sync_file(&file).map_err(|err| Error::io("sync", path, err))
}

/// Syncs the folder `dir`.
pub(super) fn sync_dir(dir: &Path) -> Result<(), Error> {
    fsync::sync_dir(dir).map_err(|err| Error::io("sync", dir, err))
}

/// The modification time of the open file `file` at `path`.
pub(super) fn modified(file: &File, path: &Path) -> Result<FileTime, Error> {
    file.metadata()
        .and_then(|metadata| metadata.modified())
        .map(FileTime::from_system)
        .map_err(|err| Error::io("stat", path, err))
}

impl GixStore {
    /// The store of the git dir `git_dir`, which exists.
    fn at(git_dir: &Path) -> Self {
        let objects = git_dir.join("objects");
        let options = |compression| loose::Options {
            alloc_limit_bytes: None,
            compression,
        };
        Self {
            git_dir: git_dir.to_path_buf(),
            stored: loose::Store::at_opts(&objects, HashKind::Sha1, options(Compression::NONE)),
            deflated: loose::Store::at_opts(
                objects,
                HashKind::Sha1,
                options(Compression::BEST_SPEED),
            ),
            blobs: Blobs::new(),
            new_objects: NewObjects::default(),
        }
    }

    /// Sets the size above which files are streamed.
    #[cfg(test)]
    pub(super) fn set_stream_threshold(&mut self, bytes: u64) {
        self.blobs.set_stream_threshold(bytes);
    }

    /// The object files written since the last publish.
    #[cfg(test)]
    pub(super) fn new_object_files(&self) -> Vec<PathBuf> {
        self.new_objects.files()
    }
}

impl HistoryStore for GixStore {
    fn create(git_dir: &Path, worktree: &Path) -> Result<Self, Error> {
        let config = config_text(worktree)?;
        let parent = git_dir
            .parent()
            .ok_or_else(|| Error::io("create", git_dir, io::ErrorKind::InvalidInput.into()))?;
        write::create_dir_all(parent).map_err(|err| Error::io("create", parent, err))?;
        match write::create_dir(git_dir) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                return Err(Error::HistoryExists {
                    git_dir: git_dir.to_path_buf(),
                });
            }
            Err(err) => return Err(Error::io("create", git_dir, err)),
        }
        new_file(&git_dir.join("HEAD"), head_text().as_bytes())?;
        new_file(&git_dir.join("config"), config.as_bytes())?;
        for name in ["objects", "refs"] {
            let dir = git_dir.join(name);
            write::create_dir(&dir).map_err(|err| Error::io("create", &dir, err))?;
        }
        sync_dir(git_dir)?;
        sync_dir(parent)?;
        if let Some(root) = parent.parent() {
            sync_dir(root)?;
        }
        Self::open(git_dir, worktree)
    }

    fn open(git_dir: &Path, worktree: &Path) -> Result<Self, Error> {
        let options = gix::open::Options::isolated().open_path_as_is(true);
        let repo = gix::open_opts(git_dir, options).map_err(|err| Error::history("open", err))?;
        let current = repo.config_snapshot().string("core.worktree");
        drop(repo);
        let head = git_dir.join("HEAD");
        let text = head_text();
        let head_now = std::fs::read(&head).map_err(|err| Error::io("read", &head, err))?;
        if head_now != text.as_bytes() {
            write::replace(&head, &mut |file| {
                file.write_all(text.as_bytes())
                    .map_err(|err| Error::io("write", &head, err))
            })?;
        }
        let wanted = worktree.to_str().map(str::as_bytes);
        if current.as_deref().map(Vec::as_slice) != wanted {
            let config = config_text(worktree)?;
            let path = git_dir.join("config");
            write::replace(&path, &mut |file| {
                file.write_all(config.as_bytes())
                    .map_err(|err| Error::io("write", &path, err))
            })?;
        }
        let mut store = Self::at(git_dir);
        store
            .new_objects
            .set_published_at(durable::ref_time(git_dir)?);
        Ok(store)
    }

    fn store_file(&mut self, file: &Path, len: u64, cap: u64) -> Result<(Oid, u64), Error> {
        let target = Target {
            stored: &self.stored,
            deflated: &self.deflated,
            new_objects: &mut self.new_objects,
        };
        self.blobs.read(file, len, cap, Some(target))
    }

    fn hash_file(&mut self, file: &Path, len: u64) -> Result<(Oid, u64), Error> {
        self.blobs.read(file, len, u64::MAX, None)
    }

    fn read_blob(&self, blob: Oid, out: &mut dyn Write) -> Result<u64, Error> {
        let id = to_gix(blob);
        let path = self.deflated.object_path(&id);
        let file = File::open(&path).map_err(|err| Error::io("open", &path, err))?;
        let mut input = BufReader::with_capacity(READ_CHUNK, file);
        let mut state = gix::zlib::Decompress::new();
        let mut chunk = vec![0_u8; READ_CHUNK];
        let mut inflate = |chunk: &mut [u8]| {
            gix::zlib::stream::inflate::read(&mut input, &mut state, chunk)
                .map_err(|err| Error::io("read", &path, err))
        };
        let mut last = inflate(&mut chunk)?;
        let first = chunk.get(..last).unwrap_or_default();
        let (kind, size, header) = gix::objs::decode::loose_header(first)
            .map_err(|err| Error::history("read blob", err))?;
        if kind != Kind::Blob {
            return Err(Error::history("read blob", format!("{id} is not a blob")));
        }
        let body = first.get(header..).unwrap_or_default();
        out.write_all(body)
            .map_err(|err| Error::history("write blob", err))?;
        let mut written = count(body.len());
        while last == chunk.len() {
            last = inflate(&mut chunk)?;
            let part = chunk.get(..last).unwrap_or_default();
            out.write_all(part)
                .map_err(|err| Error::history("write blob", err))?;
            written += count(last);
        }
        if written != size {
            let message = format!("{id} holds {written} bytes, its header says {size}");
            return Err(Error::history("read blob", message));
        }
        Ok(written)
    }

    fn blob_len(&self, blob: Oid) -> Result<u64, Error> {
        let id = to_gix(blob);
        let header = self
            .deflated
            .try_header(&id)
            .map_err(|err| Error::history("read object", err))?;
        match header {
            Some((size, Kind::Blob)) => Ok(size),
            Some(_) => Err(Error::history("read object", format!("{id} is not a blob"))),
            None => Err(Error::history("read object", format!("{id} is missing"))),
        }
    }

    fn write_tree(&mut self, files: &[TreeFile]) -> Result<Oid, Error> {
        for pair in files.windows(2) {
            if let [before, after] = pair
                && before.path.as_bytes() >= after.path.as_bytes()
            {
                let message = format!("{:?} is not before {:?}", before.path, after.path);
                return Err(Error::history("write tree", message));
            }
        }
        let id = tree::write(&self.deflated, &mut self.new_objects, files, 0)?;
        to_oid(&id)
    }

    fn read_tree(&self, tree: Oid) -> Result<Vec<TreeFile>, Error> {
        let mut files = Vec::new();
        tree::read(&self.deflated, to_gix(tree), "", &mut files)?;
        files.sort_unstable_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
        Ok(files)
    }

    fn file_at(&self, tree: Oid, path: &str) -> Result<Option<Oid>, Error> {
        tree::file_at(&self.deflated, to_gix(tree), path)
    }

    fn changed_files(&self, a: Oid, b: Oid) -> Result<(Vec<TreeFile>, Vec<TreeFile>), Error> {
        let (mut old, mut new) = (Vec::new(), Vec::new());
        tree::changed(&self.deflated, to_gix(a), to_gix(b), &mut old, &mut new)?;
        old.sort_unstable_by(|x, y| x.path.as_bytes().cmp(y.path.as_bytes()));
        new.sort_unstable_by(|x, y| x.path.as_bytes().cmp(y.path.as_bytes()));
        Ok((old, new))
    }

    fn write_commit(
        &mut self,
        tree: Oid,
        parent: Option<Oid>,
        message: &str,
        seconds: i64,
    ) -> Result<Oid, Error> {
        let parent = parent
            .map(|oid| format!("parent {}\n", oid.to_hex()))
            .unwrap_or_default();
        let data = format!(
            "tree {}\n{parent}author Clauseo <> {seconds} +0000\n\
             committer Clauseo <> {seconds} +0000\n\n{message}",
            tree.to_hex()
        );
        let id = gix::objs::compute_hash(HashKind::Sha1, Kind::Commit, data.as_bytes())
            .map_err(|err| Error::history("hash", err))?;
        put(
            &self.deflated,
            &mut self.new_objects,
            Kind::Commit,
            data.as_bytes(),
            id,
        )?;
        to_oid(&id)
    }

    fn timeline(&self) -> Result<Vec<CommitRecord>, Error> {
        let mut records = Vec::new();
        let mut next = durable::read_ref(&self.git_dir)?;
        let mut buffer = Vec::new();
        while let Some(oid) = next {
            let data = tree::find(&self.deflated, &to_gix(oid), Kind::Commit, &mut buffer)?;
            let commit = CommitRef::from_bytes(data, HashKind::Sha1)
                .map_err(|err| Error::history("read commit", err))?;
            records.push(CommitRecord {
                id: oid,
                tree: to_oid(&commit.tree())?,
                message: String::from_utf8_lossy(commit.message).into_owned(),
            });
            next = commit.parents().next().map(|id| to_oid(&id)).transpose()?;
        }
        records.reverse();
        Ok(records)
    }

    fn load_cache(&self) -> Result<Option<StatCache>, Error> {
        index_file::load(&self.git_dir.join("index"))
    }

    fn publish(
        &mut self,
        commit: Oid,
        expected: Option<Oid>,
        cache: &mut StatCache,
    ) -> Result<(), Error> {
        if !self.deflated.contains(&to_gix(commit)) {
            let message = format!("commit {} is not stored", commit.to_hex());
            return Err(Error::history("publish", message));
        }
        durable::check_ref(&self.git_dir, expected)?;
        self.new_objects.make_durable(self.deflated.path())?;
        durable::move_ref(&self.git_dir, commit, expected)?;
        self.new_objects
            .set_published_at(durable::ref_time(&self.git_dir)?);
        let index = self.git_dir.join("index");
        index_file::save(&index, cache)?;
        let file = File::open(&index).map_err(|err| Error::io("open", &index, err))?;
        fsync::full_flush(&file).map_err(|err| Error::io("sync", &index, err))?;
        cache.written_at = Some(modified(&file, &index)?);
        self.new_objects.clear();
        Ok(())
    }

    fn save_cache(&mut self, cache: &mut StatCache) -> Result<(), Error> {
        let index = self.git_dir.join("index");
        index_file::save(&index, cache)?;
        let file = File::open(&index).map_err(|err| Error::io("open", &index, err))?;
        cache.written_at = Some(modified(&file, &index)?);
        Ok(())
    }
}
