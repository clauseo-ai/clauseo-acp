//! Checks of a loose object file found already present: its header alone, or its whole content
//! inflated and hashed. A file that does not decode is unsound (`false`); an I/O error reading
//! it is an error, so a transient failure never counts as a torn object. And [`same`], the
//! check every object read from history passes.

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use gix::hash::{Kind as HashKind, ObjectId};
use gix::objs::Kind;
use gix::zlib::{Decompress, FlushDecompress, Status};

use crate::Error;

/// The compressed bytes read to find a header: more than any loose header deflates to.
const HEADER_INPUT: u64 = 256;
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

/// Whether the file at `path` inflates to a header of `kind` and `size`, reading at most a
/// few hundred bytes.
pub(super) fn header(path: &Path, kind: Kind, size: u64) -> Result<bool, Error> {
    let mut input = Vec::new();
    open(path)?
        .take(HEADER_INPUT)
        .read_to_end(&mut input)
        .map_err(|err| Error::io("read", path, err))?;
    let mut state = Decompress::new();
    let mut out = [0_u8; HEADER_MAX];
    if state
        .decompress(&input, &mut out, FlushDecompress::None)
        .is_err()
    {
        return Ok(false);
    }
    let produced = usize::try_from(state.total_out()).unwrap_or(0);
    let head = out.get(..produced).unwrap_or_default();
    Ok(header_len(head, kind, size).is_some())
}

/// Whether the file at `path` inflates in full to an object of `kind` and `size` whose hash
/// is `id`.
pub(super) fn whole(path: &Path, id: &ObjectId, kind: Kind, size: u64) -> Result<bool, Error> {
    let mut input = BufReader::with_capacity(CHUNK, open(path)?);
    let mut state = Decompress::new();
    let mut out = vec![0_u8; CHUNK];
    let mut hasher = gix::hash::hasher(HashKind::Sha1);
    let mut head = Vec::with_capacity(HEADER_MAX);
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
            return Ok(false);
        };
        let consumed = usize::try_from(state.total_in() - before_in).unwrap_or(0);
        let written = usize::try_from(state.total_out() - before_out).unwrap_or(0);
        input.consume(consumed);
        let produced = out.get(..written).unwrap_or_default();
        hasher.update(produced);
        head.extend(produced.iter().take(HEADER_MAX - head.len()));
        match status {
            Status::StreamEnd => break,
            Status::Ok | Status::BufError if eof || (consumed == 0 && written == 0) => {
                return Ok(false);
            }
            Status::Ok | Status::BufError => {}
        }
    }
    let Some(len) = header_len(&head, kind, size) else {
        return Ok(false);
    };
    let total = u64::try_from(len).unwrap_or(u64::MAX).saturating_add(size);
    Ok(state.total_out() == total && hasher.try_finalize().is_ok_and(|found| found == *id))
}

/// Nothing when `found`, the hash of an object read for `id`, is `id`; otherwise a history
/// error of `op` that names the object and `path`, the file or folder it was read for (empty
/// for the root tree or a commit).
pub(super) fn same(
    id: &ObjectId,
    found: ObjectId,
    op: &'static str,
    path: &str,
) -> Result<(), Error> {
    if found == *id {
        return Ok(());
    }
    let at = if path.is_empty() {
        String::new()
    } else {
        format!("{path}: ")
    };
    let message = format!("{at}{id} does not match its content (hashes to {found})");
    Err(Error::history(op, message))
}
