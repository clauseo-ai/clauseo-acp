//! The checks of a loose object file found already present, each inflating it in full: against
//! the bytes in hand when the checkpoint holds them, else hashed against its id. A file that
//! does not decode is unsound (`false`); an I/O error reading it is an error, so a transient
//! failure never counts as a torn object.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use gix::hash::{Kind as HashKind, ObjectId};
use gix::objs::Kind;
use gix::zlib::{Decompress, FlushDecompress, Status};

use crate::Error;

/// The most bytes a loose header inflates to (`gix` allows the same).
const HEADER_MAX: usize = 64;
/// The chunk in which a whole object is inflated.
const CHUNK: usize = 64 * 1024;

fn open(path: &Path) -> Result<File, Error> {
    File::open(path).map_err(|err| Error::io("open", path, err))
}

/// Whether `head` starts with a loose header of `kind` and `size`; its length if so.
fn header_len(head: &[u8], kind: Kind, size: u64) -> Option<usize> {
    match gix::objs::decode::loose_header(head) {
        Ok((found, found_size, len)) if found == kind && found_size == size => Some(len),
        Ok(_) | Err(_) => None,
    }
}

/// Inflates the file at `path` in full, passing each run of bytes it gives to `sink`, which
/// returns `false` to stop. The bytes inflated when the stream ended, or `None` when it did not
/// decode, ended early, or `sink` stopped it.
fn inflate(path: &Path, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Option<u64>, Error> {
    let mut input = BufReader::with_capacity(CHUNK, open(path)?);
    let mut state = Decompress::new();
    let mut out = vec![0_u8; CHUNK];
    loop {
        let data = input
            .fill_buf()
            .map_err(|err| Error::io("read", path, err))?;
        let eof = data.is_empty();
        let flush = if eof {
            FlushDecompress::Finish
        } else {
            FlushDecompress::None
        };
        let (before_in, before_out) = (state.total_in(), state.total_out());
        let Ok(status) = state.decompress(data, &mut out, flush) else {
            return Ok(None);
        };
        let consumed = usize::try_from(state.total_in() - before_in).unwrap_or(0);
        let written = usize::try_from(state.total_out() - before_out).unwrap_or(0);
        input.consume(consumed);
        if !sink(out.get(..written).unwrap_or_default()) {
            return Ok(None);
        }
        match status {
            Status::StreamEnd => return Ok(Some(state.total_out())),
            Status::Ok | Status::BufError if eof || (consumed == 0 && written == 0) => {
                return Ok(None);
            }
            Status::Ok | Status::BufError => {}
        }
    }
}

/// Whether the file at `path` inflates in full to an object of `kind` and `size` whose hash
/// is `id`: the check of an object whose bytes are not in hand (a streamed file).
pub(super) fn whole(path: &Path, id: &ObjectId, kind: Kind, size: u64) -> Result<bool, Error> {
    let mut hasher = gix::hash::hasher(HashKind::Sha1);
    let mut head = Vec::with_capacity(HEADER_MAX);
    let total = inflate(path, &mut |produced| {
        hasher.update(produced);
        head.extend(produced.iter().take(HEADER_MAX - head.len()));
        true
    })?;
    let Some(len) = header_len(&head, kind, size) else {
        return Ok(false);
    };
    let expected = u64::try_from(len).unwrap_or(u64::MAX).saturating_add(size);
    Ok(total == Some(expected) && hasher.try_finalize().is_ok_and(|found| found == *id))
}

/// Whether the file at `path` inflates in full to the loose header of `kind` and `data`'s
/// length followed by exactly `data`: the check of an object whose bytes are in hand, which
/// needs no hash (the id was computed from `data`). Stops at the first byte that differs.
pub(super) fn same(path: &Path, kind: Kind, data: &[u8]) -> Result<bool, Error> {
    let header = gix::objs::encode::loose_header(kind, super::blob::count(data.len()));
    let mut at = 0_usize;
    let total = inflate(path, &mut |produced| {
        let mut rest = produced;
        while !rest.is_empty() {
            // What should come next: the rest of the header, else the rest of `data`.
            let expected = match at.checked_sub(header.len()) {
                None => header.get(at..),
                Some(offset) => data.get(offset..),
            }
            .unwrap_or_default();
            let n = rest.len().min(expected.len());
            if n == 0 || rest.get(..n) != expected.get(..n) {
                return false;
            }
            rest = rest.get(n..).unwrap_or_default();
            at += n;
        }
        true
    })?;
    Ok(total == Some(super::blob::count(header.len() + data.len())))
}
