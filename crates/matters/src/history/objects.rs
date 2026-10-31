//! [`Objects`]: the one home of the loose object store. Every read that hands out content checks
//! it against the id it was asked for, so a well-formed object of other bytes (a disk fault, a
//! sync tool, a bad copy of the history folder) never reaches a caller; gix's loose `try_find`
//! checks the header and the length but not the hash. Writes go through [`Writer`], which
//! cannot read, and nothing else in the crate holds a `loose::Store`.

use std::cell::RefCell;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use gix::hash::{Kind as HashKind, ObjectId};
use gix::objs::{Kind, Write as _};
use gix::odb::loose;
use gix::zlib::Compression;

use super::blob::count;
use crate::Error;

/// The chunk in which a blob is inflated by [`Objects::read_blob`].
const READ_CHUNK: usize = 64 * 1024;

/// The operation every read error of this module names.
const OP: &str = "read object";

/// The loose objects of one history: two stores on one `objects` folder.
pub(super) struct Objects {
    /// Writes blobs that are already compressed, at zlib level 0.
    stored: loose::Store,
    /// Writes every other object at zlib level 1, and locates every object.
    deflated: loose::Store,
}

/// A loose store that writes objects and names their files, and reads nothing.
#[derive(Clone, Copy)]
pub(super) struct Writer<'s>(&'s loose::Store);

impl Writer<'_> {
    /// The file of object `id`.
    pub(super) fn object_path(self, id: &gix::oid) -> PathBuf {
        self.0.object_path(id)
    }

    /// Writes object `id` (`kind`, `data`).
    pub(super) fn write_buf(self, kind: Kind, data: &[u8], id: ObjectId) -> Result<(), Error> {
        self.0
            .write_buf_with_known_id(kind, data, id)
            .map(drop)
            .map_err(|err| Error::history("write object", err))
    }

    /// Writes object `id` (`kind`, `size` bytes from `from`); gix's error as it is, so the
    /// caller can tell a failing source from a failing write.
    pub(super) fn write_stream(
        self,
        kind: Kind,
        size: u64,
        from: &mut dyn std::io::Read,
        id: ObjectId,
    ) -> gix::Result<ObjectId> {
        self.0.write_stream_with_known_id(kind, size, from, id)
    }
}

/// Nothing when `found`, the hash of what was read for `id`, is `id`; otherwise a history error
/// that names the object and `path`, the file or folder it was read for (empty when unknown).
fn same(id: &ObjectId, found: ObjectId, path: &str) -> Result<(), Error> {
    if found == *id {
        return Ok(());
    }
    Err(Error::history(
        OP,
        format!(
            "{}{id} does not match its content (hashes to {found})",
            at(path)
        ),
    ))
}

/// `path` and `: `, or nothing for an empty path: how an error names where it was.
fn at(path: &str) -> String {
    if path.is_empty() {
        String::new()
    } else {
        format!("{path}: ")
    }
}

impl Objects {
    /// The stores of the folder `objects`.
    pub(super) fn at(objects: &Path) -> Self {
        let options = |compression| loose::Options {
            alloc_limit_bytes: None,
            compression,
        };
        Self {
            stored: loose::Store::at_opts(objects, HashKind::Sha1, options(Compression::NONE)),
            deflated: loose::Store::at_opts(
                objects,
                HashKind::Sha1,
                options(Compression::BEST_SPEED),
            ),
        }
    }

    /// Writes at zlib level 0.
    pub(super) const fn stored(&self) -> Writer<'_> {
        Writer(&self.stored)
    }

    /// Writes at zlib level 1.
    pub(super) const fn deflated(&self) -> Writer<'_> {
        Writer(&self.deflated)
    }

    /// The `objects` folder.
    pub(super) fn path(&self) -> &Path {
        self.deflated.path()
    }

    /// Whether the file of object `id` exists; its content is not read.
    pub(super) fn contains(&self, id: &gix::oid) -> bool {
        self.deflated.contains(id)
    }

    /// The size and kind of object `id` from its header alone, unchecked: a size, never content.
    pub(super) fn header(&self, id: &gix::oid) -> Result<(u64, Kind), Error> {
        self.deflated
            .try_header(id)
            .map_err(|err| Error::history(OP, err))?
            .ok_or_else(|| Error::history(OP, format!("{id} is missing")))
    }

    /// The content of object `id`, which must be of `kind` and hash to `id`, read into
    /// `buffer`; `path` is the folder it was read for (empty for the root or a commit).
    pub(super) fn find<'a>(
        &self,
        id: &ObjectId,
        kind: Kind,
        path: &str,
        buffer: &'a mut Vec<u8>,
    ) -> Result<&'a [u8], Error> {
        let (found, data) = self.find_any(id, path, buffer)?;
        if found != kind {
            let message = format!("{}{id} is not a {kind}", at(path));
            return Err(Error::history(OP, message));
        }
        Ok(data)
    }

    /// The kind and content of object `id`, checked against `id`.
    fn find_any<'a>(
        &self,
        id: &ObjectId,
        path: &str,
        buffer: &'a mut Vec<u8>,
    ) -> Result<(Kind, &'a [u8]), Error> {
        let data = self
            .deflated
            .try_find(id, buffer)
            .map_err(|err| Error::history(OP, err))?
            .ok_or_else(|| Error::history(OP, format!("{}{id} is missing", at(path))))?;
        let found = gix::objs::compute_hash(HashKind::Sha1, data.kind, data.data)
            .map_err(|err| Error::history(OP, err))?;
        same(id, found, path)?;
        Ok((data.kind, data.data))
    }

    /// Writes the content of blob `id`, read for the file `path`, to `out`, hashing the header
    /// and the body as inflated in the pass that writes them; returns the bytes written. An
    /// object that does not hash to `id` is an error after its last byte, so `out` must be a
    /// staging place its caller drops on an error.
    pub(super) fn read_blob(
        &self,
        id: &ObjectId,
        path: &str,
        out: &mut dyn Write,
    ) -> Result<u64, Error> {
        let object = self.deflated.object_path(id);
        let file = File::open(&object).map_err(|err| Error::io("open", &object, err))?;
        let mut input = BufReader::with_capacity(READ_CHUNK, file);
        let mut state = gix::zlib::Decompress::new();
        let mut chunk = vec![0_u8; READ_CHUNK];
        let mut inflate = |chunk: &mut [u8]| {
            gix::zlib::stream::inflate::read(&mut input, &mut state, chunk)
                .map_err(|err| Error::io("read", &object, err))
        };
        let mut last = inflate(&mut chunk)?;
        let first = chunk.get(..last).unwrap_or_default();
        let (kind, size, header) =
            gix::objs::decode::loose_header(first).map_err(|err| Error::history(OP, err))?;
        if kind != Kind::Blob {
            let message = format!("{}{id} is not a blob", at(path));
            return Err(Error::history(OP, message));
        }
        let mut hasher = gix::hash::hasher(HashKind::Sha1);
        hasher.update(first.get(..header).unwrap_or_default());
        let body = first.get(header..).unwrap_or_default();
        hasher.update(body);
        out.write_all(body)
            .map_err(|err| Error::history("write blob", err))?;
        let mut written = count(body.len());
        while last == chunk.len() {
            last = inflate(&mut chunk)?;
            let part = chunk.get(..last).unwrap_or_default();
            hasher.update(part);
            out.write_all(part)
                .map_err(|err| Error::history("write blob", err))?;
            written += count(last);
        }
        if written != size {
            let message = format!(
                "{}{id} holds {written} bytes, its header says {size}",
                at(path)
            );
            return Err(Error::history(OP, message));
        }
        let found = hasher
            .try_finalize()
            .map_err(|err| Error::history(OP, err))?;
        same(id, found, path)?;
        Ok(written)
    }

    /// Reads objects for `gix::diff::tree` through [`Objects::find_any`]; [`Finder::failed`]
    /// gives back the first error in this module's words, which gix's would wrap.
    pub(super) fn finder(&self) -> Finder<'_> {
        Finder {
            objects: self,
            failed: RefCell::new(None),
        }
    }
}

/// [`Objects::finder`].
pub(super) struct Finder<'s> {
    objects: &'s Objects,
    failed: RefCell<Option<Error>>,
}

impl Finder<'_> {
    /// The first read error, if any.
    pub(super) fn failed(self) -> Option<Error> {
        self.failed.into_inner()
    }
}

impl gix::objs::Find for Finder<'_> {
    fn try_find<'b>(
        &self,
        id: &gix::oid,
        buffer: &'b mut Vec<u8>,
    ) -> gix::Result<Option<gix::objs::Data<'b>>> {
        match self.objects.find_any(&id.to_owned(), "", buffer) {
            Ok((kind, data)) => Ok(Some(gix::objs::Data {
                kind,
                object_hash: HashKind::Sha1,
                data,
            })),
            Err(err) => {
                let message = err.to_string();
                self.failed.borrow_mut().get_or_insert(err);
                Err(gix::Error::from_error(std::io::Error::other(message)))
            }
        }
    }
}
