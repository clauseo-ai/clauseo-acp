//! Files as blobs: hashed with gix's SHA-1 and, when asked, stored as loose objects at the
//! level the first bytes call for. Files up to a threshold are read whole into a reused
//! buffer; larger ones are hashed in chunks, then streamed into the store.

use std::fs::{File, Metadata};
use std::io::{self, Read, Seek};
use std::path::Path;

use super::objects::Writer;
use gix::hash::{Kind as HashKind, ObjectId};
use gix::objs::Kind;

use super::Oid;
use super::durable::{self, NewObjects, put};
use super::gix_store::to_oid;
use crate::{Error, write};

/// Files up to this size are read whole into a reused buffer; larger ones are streamed.
const STREAM_THRESHOLD: u64 = 8 * 1024 * 1024;
/// The chunk in which a streamed file is hashed.
const HASH_CHUNK: usize = 1024 * 1024;
/// How many times a file that changes while it is read is read before giving up.
const ATTEMPTS: u32 = 3;
/// How many first bytes [`is_already_compressed`] looks at.
const HEAD_BYTES: usize = 12;

/// Whether `head`, a file's first bytes, names a format that is already compressed: zip (and
/// so docx, xlsx, pptx), PDF, JPEG, PNG, GIF, WebP and HEIC (any `ftyp` box).
pub(crate) fn is_already_compressed(head: &[u8]) -> bool {
    const PREFIXES: [&[u8]; 5] = [b"PK\x03\x04", b"%PDF", b"\xFF\xD8\xFF", b"\x89PNG", b"GIF8"];
    PREFIXES.iter().any(|prefix| head.starts_with(prefix))
        || (head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP".as_slice()))
        || head.get(4..8) == Some(b"ftyp".as_slice())
}

/// A byte count as `u64`.
pub(super) fn count(bytes: usize) -> u64 {
    u64::try_from(bytes).unwrap_or(u64::MAX)
}

/// Where a blob is stored: two loose stores on one `objects` folder, and the record of new
/// object files.
pub(super) struct Target<'a> {
    /// Writes blobs that are already compressed, at zlib level 0.
    pub(super) stored: Writer<'a>,
    /// Writes every other blob, at zlib level 1.
    pub(super) deflated: Writer<'a>,
    /// Object files written since the last publish.
    pub(super) new_objects: &'a mut NewObjects,
}

impl<'a> Target<'a> {
    /// The store for a blob whose first bytes are `head`.
    fn store_for(&self, head: &[u8]) -> Writer<'a> {
        if is_already_compressed(head) {
            self.stored
        } else {
            self.deflated
        }
    }
}

/// The file at `path` opened without following a symlink; `None` when the open file is not a
/// regular file or (on Unix) not the one `symlink_metadata` of `path` names.
fn open_checked(path: &Path) -> Result<Option<File>, Error> {
    let file = write::open_read_nofollow(path).map_err(|err| Error::io("open", path, err))?;
    let opened = file
        .metadata()
        .map_err(|err| Error::io("stat", path, err))?;
    let named = std::fs::symlink_metadata(path).map_err(|err| Error::io("stat", path, err))?;
    let same = opened.is_file() && named.is_file() && same_file(&opened, &named);
    Ok(same.then_some(file))
}

/// Whether two metadata name one file: same device and inode.
#[cfg(unix)]
fn same_file(a: &Metadata, b: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// Windows: std exposes no file id on stable, so only the regular-file checks apply.
#[cfg(not(unix))]
fn same_file(_: &Metadata, _: &Metadata) -> bool {
    true
}

/// The reader of files as blobs, with its reused buffer.
pub(super) struct Blobs {
    /// The buffer files are read into, reused across calls.
    buffer: Vec<u8>,
    /// Files larger than this are streamed rather than read whole.
    stream_threshold: u64,
}

impl Blobs {
    pub(super) const fn new() -> Self {
        Self {
            buffer: Vec::new(),
            stream_threshold: STREAM_THRESHOLD,
        }
    }

    /// Sets the size above which files are streamed.
    #[cfg(test)]
    pub(super) fn set_stream_threshold(&mut self, bytes: u64) {
        self.stream_threshold = bytes;
    }

    /// Hashes the file `path` (`len` bytes by its metadata) as a blob, storing it in `target`
    /// if given; returns the blob and the bytes read. A file whose length is not `len`, that
    /// changes between the two passes of a stream, or whose open file is not the file at
    /// `path` (a symlink or another file swapped in), is opened, stat'd and read again, at most
    /// [`ATTEMPTS`] times in all; a length over `cap` (the walk's
    /// [`crate::Limits::effective_cap`]) is refused as an [`Error::Io`] of kind
    /// `FileTooLarge`, and nothing is stored.
    pub(super) fn read(
        &mut self,
        path: &Path,
        len: u64,
        cap: u64,
        mut target: Option<Target<'_>>,
    ) -> Result<(Oid, u64), Error> {
        let mut expected = len;
        for attempt in 0..ATTEMPTS {
            let Some(mut file) = open_checked(path)? else {
                continue;
            };
            if attempt > 0 {
                let metadata = file
                    .metadata()
                    .map_err(|err| Error::io("stat", path, err))?;
                expected = metadata.len();
            }
            if expected > cap {
                let over = io::Error::new(io::ErrorKind::FileTooLarge, "the file is over the cap");
                return Err(Error::io("read", path, over));
            }
            let done = if expected <= self.stream_threshold {
                self.buffered(&mut file, path, expected, target.as_mut())?
            } else {
                self.streamed(&mut file, path, expected, target.as_mut())?
            };
            if let Some(done) = done {
                return Ok(done);
            }
        }
        let changed = io::Error::new(io::ErrorKind::InvalidData, "the file kept changing");
        Err(Error::io("read", path, changed))
    }

    /// Reads `file` whole into the buffer, hashes it and stores it; `None` when it did not
    /// hold exactly `len` bytes.
    fn buffered(
        &mut self,
        file: &mut File,
        path: &Path,
        len: u64,
        target: Option<&mut Target<'_>>,
    ) -> Result<Option<(Oid, u64)>, Error> {
        self.buffer.clear();
        let read = file
            .take(len.saturating_add(1))
            .read_to_end(&mut self.buffer)
            .map_err(|err| Error::io("read", path, err))?;
        if count(read) != len {
            return Ok(None);
        }
        let id = gix::objs::compute_hash(HashKind::Sha1, Kind::Blob, &self.buffer)
            .map_err(|err| Error::history("hash", err))?;
        if let Some(target) = target {
            let store = target.store_for(&self.buffer);
            put(store, target.new_objects, Kind::Blob, &self.buffer, id)?;
        }
        Ok(Some((to_oid(&id)?, len)))
    }

    /// Hashes `file` in chunks, then streams it into the store if it is new; `None` when it
    /// did not hold exactly `len` bytes or changed between the two passes.
    fn streamed(
        &mut self,
        file: &mut File,
        path: &Path,
        len: u64,
        target: Option<&mut Target<'_>>,
    ) -> Result<Option<(Oid, u64)>, Error> {
        self.buffer.clear();
        self.buffer.resize(HASH_CHUNK, 0);
        let mut hasher = gix::hash::hasher(HashKind::Sha1);
        hasher.update(&gix::objs::encode::loose_header(Kind::Blob, len));
        let mut head = Vec::with_capacity(HEAD_BYTES);
        let mut read = 0_u64;
        let mut limited = Read::by_ref(file).take(len.saturating_add(1));
        loop {
            let n = match limited.read(&mut self.buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(Error::io("read", path, err)),
            };
            let chunk = self.buffer.get(..n).unwrap_or_default();
            head.extend(chunk.iter().take(HEAD_BYTES - head.len()));
            hasher.update(chunk);
            read += count(n);
        }
        if read != len {
            return Ok(None);
        }
        let id = hasher
            .try_finalize()
            .map_err(|err| Error::history("hash", err))?;
        let Some(target) = target else {
            return Ok(Some((to_oid(&id)?, len)));
        };
        if durable::present(target.deflated, target.new_objects, &id, Kind::Blob, len)? {
            return Ok(Some((to_oid(&id)?, len)));
        }
        file.rewind().map_err(|err| Error::io("read", path, err))?;
        let object = target.deflated.object_path(&id);
        let made_folder = !object.parent().is_some_and(Path::is_dir);
        let mut source = Checked::new(file, len, id);
        let written = target
            .store_for(&head)
            .write_stream(Kind::Blob, len, &mut source, id);
        match written {
            Ok(_) => {
                target.new_objects.record(object, made_folder);
                Ok(Some((to_oid(&id)?, len)))
            }
            Err(_) if source.changed => Ok(None),
            Err(err) => Err(Error::history("write object", err)),
        }
    }
}

/// The second pass over a streamed file: yields at most `remaining` bytes, hashing them, and
/// fails (setting `changed`) if the file ends early, goes on past them, or hashes differently
/// from the first pass, so a changed file is never stored.
struct Checked<'a> {
    file: &'a mut File,
    remaining: u64,
    hasher: Option<gix::hash::Hasher>,
    expected: ObjectId,
    changed: bool,
}

impl<'a> Checked<'a> {
    fn new(file: &'a mut File, len: u64, expected: ObjectId) -> Self {
        let mut hasher = gix::hash::hasher(HashKind::Sha1);
        hasher.update(&gix::objs::encode::loose_header(Kind::Blob, len));
        Self {
            file,
            remaining: len,
            hasher: Some(hasher),
            expected,
            changed: false,
        }
    }

    fn changed(&mut self) -> io::Error {
        self.changed = true;
        io::Error::new(
            io::ErrorKind::InvalidData,
            "the file changed while it was stored",
        )
    }

    /// At the end: checks that the file ends here and hashes as the first pass did.
    fn finish(&mut self) -> io::Result<usize> {
        let Some(hasher) = self.hasher.take() else {
            return Ok(0);
        };
        let mut extra = [0_u8; 1];
        if self.file.read(&mut extra)? != 0 {
            return Err(self.changed());
        }
        match hasher.try_finalize() {
            Ok(id) if id == self.expected => Ok(0),
            Ok(_) => Err(self.changed()),
            Err(err) => Err(io::Error::other(err)),
        }
    }
}

impl Read for Checked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return self.finish();
        }
        let limit = usize::try_from(self.remaining)
            .unwrap_or(usize::MAX)
            .min(buf.len());
        let Some(window) = buf.get_mut(..limit) else {
            return Ok(0);
        };
        let n = self.file.read(window)?;
        if n == 0 {
            return Err(self.changed());
        }
        if let (Some(hasher), Some(bytes)) = (self.hasher.as_mut(), window.get(..n)) {
            hasher.update(bytes);
        }
        self.remaining = self.remaining.saturating_sub(count(n));
        Ok(n)
    }
}
