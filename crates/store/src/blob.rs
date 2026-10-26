//! Blobs: the hash and preview of a payload, and ranged reads through SQLite's incremental
//! blob I/O.

use clauseo_protocol::{BlobHash, BlobRef, MAX_BLOB_READ_BYTES, PREVIEW_BYTES};
use rusqlite::{Connection, MAIN_DB, OptionalExtension};

use crate::Error;
use crate::model::blob_ref_of;

/// The reference an event carries for `bytes`: their BLAKE3 hash, their size and a preview,
/// the longest UTF-8 prefix of at most [`PREVIEW_BYTES`]. [`BlobIn::blob_ref`] gives the
/// same value without hashing again.
///
/// [`BlobIn::blob_ref`]: crate::BlobIn::blob_ref
pub fn blob_ref(bytes: &[u8]) -> BlobRef {
    blob_ref_of(BlobHash::from_bytes(*blake3::hash(bytes).as_bytes()), bytes)
}

/// The longest prefix of `bytes` that is valid UTF-8 and at most [`PREVIEW_BYTES`] long.
pub(crate) fn preview(bytes: &[u8]) -> &str {
    let head = bytes.get(..PREVIEW_BYTES).unwrap_or(bytes);
    match std::str::from_utf8(head) {
        Ok(text) => text,
        Err(error) => head
            .get(..error.valid_up_to())
            .and_then(|valid| std::str::from_utf8(valid).ok())
            .unwrap_or_default(),
    }
}

/// Up to `len` bytes of the blob `hash` from `offset`, cut at the blob's end and at
/// [`MAX_BLOB_READ_BYTES`]. A read of the whole blob checks its hash.
pub(crate) fn read(
    conn: &Connection,
    hash: BlobHash,
    offset: u64,
    len: u32,
) -> Result<Vec<u8>, Error> {
    let found = conn
        .prepare_cached("SELECT id, size_bytes FROM blobs WHERE hash = ?1")?
        .query_row([hash.as_bytes().as_slice()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
        .optional()?;
    let Some((id, size)) = found else {
        return Err(Error::BlobMissing { hash });
    };
    let size = u64::try_from(size)
        .map_err(|error| Error::corrupt(format!("blob {hash} has size {size}: {error}")))?;
    let len = u64::from(len.min(MAX_BLOB_READ_BYTES));
    let start = offset.min(size);
    let end = start.saturating_add(len).min(size);
    let mut out = vec![0; usize::try_from(end - start).unwrap_or(0)];
    if out.is_empty() {
        return Ok(out);
    }
    let blob = conn.blob_open(MAIN_DB, c"blobs", c"data", id, true)?;
    if u64::try_from(blob.len()).ok() != Some(size) {
        return Err(Error::corrupt(format!(
            "blob {hash} holds {} bytes, its row says {size}",
            blob.len()
        )));
    }
    let start = usize::try_from(start)
        .map_err(|error| Error::corrupt(format!("blob {hash} offset {start}: {error}")))?;
    blob.read_at_exact(&mut out, start)?;
    if start == 0 && end == size && blake3::hash(&out).as_bytes() != hash.as_bytes() {
        return Err(Error::corrupt(format!(
            "blob {hash} does not match its hash"
        )));
    }
    Ok(out)
}
