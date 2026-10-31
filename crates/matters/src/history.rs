//! The shadow repository behind one trait, so `git2` could replace gix if a spike finds a
//! blocker. [`GixStore`] is the one implementation; swapping it means a second implementation
//! and changing the one field of `checkpoint::Core` that names it. Nothing outside this module
//! names a gix type.
//!
//! The repository is a bare-style git dir (`history.git`) with `core.worktree` set to the matter
//! folder, `HEAD` a symbolic ref to the unborn branch `refs/heads/clauseo`, the timeline on
//! `refs/clauseo/timeline`, loose objects only, and the stat cache as its `index` file.
//! Decision record 0007 is the format; this module writes it.

mod blob;
mod check;
mod durable;
mod gix_store;
mod index_file;
#[cfg(test)]
mod tests;
mod tree;

use std::fmt;
use std::io::Write;
use std::path::Path;

pub(crate) use gix_store::{GixStore, recorded_worktree};

use crate::Error;
use crate::stat::StatCache;

/// The ref every checkpoint moves forward. The only ref that exists besides `HEAD`.
pub(crate) const TIMELINE_REF: &str = "refs/clauseo/timeline";

/// The branch `HEAD` names. It is never born (no commit is ever made on it): a `HEAD` naming a
/// ref outside `refs/heads/` is `badHeadTarget` to `git fsck`, while an unborn branch is not.
pub(crate) const HEAD_REF: &str = "refs/heads/clauseo";

/// A git object id (SHA-1), kept free of gix's types.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Oid(pub(crate) [u8; 20]);

impl Oid {
    /// Lowercase hex, as `ContentId` carries it.
    pub(crate) fn to_hex(self) -> String {
        use fmt::Write as _;
        let mut text = String::with_capacity(40);
        for byte in self.0 {
            // Writing to a String cannot fail.
            write!(text, "{byte:02x}").unwrap_or_default();
        }
        text
    }
}

impl fmt::Debug for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Hashes files as blobs without storing anything, with its own reused buffer, so a walk that
/// writes nothing (a restore preview, a compare with the folder) needs no `&mut` on the store.
pub(crate) struct Hasher(blob::Blobs);

impl Hasher {
    pub(crate) const fn new() -> Self {
        Self(blob::Blobs::new())
    }

    /// Hashes the file `file` (`len` bytes by its metadata) as a blob; returns it and the bytes
    /// read. A length over `cap` is an [`Error::Io`] of kind `FileTooLarge`, as for
    /// [`HistoryStore::store_file`].
    pub(crate) fn hash(&mut self, file: &Path, len: u64, cap: u64) -> Result<(Oid, u64), Error> {
        self.0.read(file, len, cap, None)
    }
}

/// One commit of the timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommitRecord {
    /// The commit.
    pub(crate) id: Oid,
    /// Its tree.
    pub(crate) tree: Oid,
    /// Its whole message, trailers included.
    pub(crate) message: String,
}

/// One file of a flattened tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeFile {
    /// Its path from the tree's root, `/`-separated.
    pub(crate) path: String,
    /// Its blob.
    pub(crate) blob: Oid,
}

/// What a checkpoint needs from a git repository. Writes of objects are not durable until
/// [`HistoryStore::publish`]; nothing names them until then.
pub(crate) trait HistoryStore: Sized {
    /// Makes the git dir `git_dir`, which must not exist (its parent may not either), with
    /// `core.worktree` set to `worktree`, and opens it.
    fn create(git_dir: &Path, worktree: &Path) -> Result<Self, Error>;

    /// Opens the git dir `git_dir` isolated from every git config and environment variable,
    /// sets `core.worktree` to `worktree` if it says otherwise (the folder was moved), and
    /// rewrites `HEAD` if it holds anything but the symbolic ref to [`HEAD_REF`].
    fn open(git_dir: &Path, worktree: &Path) -> Result<Self, Error>;

    /// Hashes the file `file` as a blob and stores it as a loose object unless it is already
    /// there: zlib level 0 when its first bytes say it is already compressed, level 1
    /// otherwise. `len` is its size from the walk's metadata and `cap` the walk's size cap
    /// ([`crate::Limits::effective_cap`]): a file found over it when read is an
    /// [`Error::Io`] of kind `FileTooLarge`, and nothing is stored. Returns the blob and the
    /// bytes actually read.
    fn store_file(&mut self, file: &Path, len: u64, cap: u64) -> Result<(Oid, u64), Error>;

    /// Hashes the file `file` as a blob without storing anything.
    fn hash_file(&mut self, file: &Path, len: u64) -> Result<(Oid, u64), Error>;

    /// Writes the content of blob `blob`, read for the file `path`, to `out`; returns the bytes
    /// written. The content is hashed as it streams: an object that does not hash to `blob` is
    /// an [`Error::History`] naming it and `path`, returned after the last byte, so `out`
    /// must be a staging place (a temp file, a buffer) its caller drops on an error.
    fn read_blob(&self, blob: Oid, path: &str, out: &mut dyn Write) -> Result<u64, Error>;

    /// The size of blob `blob` in bytes, from its header.
    fn blob_len(&self, blob: Oid) -> Result<u64, Error>;

    /// Stores the trees for `files` (sorted by path bytes, `/`-separated, every file mode
    /// `100644`) and returns the root tree.
    fn write_tree(&mut self, files: &[TreeFile]) -> Result<Oid, Error>;

    /// Every file of tree `tree`, flattened and sorted by path bytes.
    fn read_tree(&self, tree: Oid) -> Result<Vec<TreeFile>, Error>;

    /// The blob of the file at `path` in tree `tree`, reading only the trees on the way; `None`
    /// when the tree has no file there.
    fn file_at(&self, tree: Oid, path: &str) -> Result<Option<Oid>, Error>;

    /// The files of trees `a` and `b` that the other does not hold with the same blob, each
    /// list sorted by path bytes, reading no subtree whose id both share. A diff of the two
    /// lists equals the diff of the whole trees.
    fn changed_files(&self, a: Oid, b: Oid) -> Result<(Vec<TreeFile>, Vec<TreeFile>), Error>;

    /// Stores a commit of `tree` with parent `parent`, author and committer `Clauseo`, at
    /// `seconds` since the epoch in UTC, with `message`.
    fn write_commit(
        &mut self,
        tree: Oid,
        parent: Option<Oid>,
        message: &str,
        seconds: i64,
    ) -> Result<Oid, Error>;

    /// The timeline's commits, oldest first, following first parents from [`TIMELINE_REF`];
    /// empty when the ref does not exist yet.
    fn timeline(&self) -> Result<Vec<CommitRecord>, Error>;

    /// The stat cache, or `None` when the file is missing or does not read (it is then
    /// rebuilt, at the cost of reading every file once).
    fn load_cache(&self) -> Result<Option<StatCache>, Error>;

    /// Makes every object written since the last publish durable, then moves [`TIMELINE_REF`]
    /// from `expected` (`None`: it must not exist) to `commit` and writes `cache`, then makes
    /// those durable: the sync order of decision record 0007. Sets `cache.written_at`.
    fn publish(
        &mut self,
        commit: Oid,
        expected: Option<Oid>,
        cache: &mut StatCache,
    ) -> Result<(), Error>;

    /// Writes `cache` alone when no commit is made, by temp file and rename with a plain sync
    /// and no full flush: it is a cache, rebuilt if lost. Sets `cache.written_at`.
    fn save_cache(&mut self, cache: &mut StatCache) -> Result<(), Error>;
}
