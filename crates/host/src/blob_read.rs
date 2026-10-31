//! Turns a `ReadBlob` query into a store read and cuts the bytes read to whole characters.

use clauseo_protocol::{BlobRange, Count, MAX_BLOB_READ_BYTES, ReadLen};

use crate::error::Error;

/// The most continuation bytes a read skips to reach a character, and the most it reads past
/// `len` to finish the last one.
const UTF8_SLACK: u32 = 6;

/// How many bytes to ask the store for: `len` plus room to skip into and finish a character,
/// at most the 1 MiB cap.
pub(crate) fn request_len(len: ReadLen) -> u32 {
    len.get()
        .saturating_add(UTF8_SLACK)
        .min(MAX_BLOB_READ_BYTES)
}

/// Cuts `bytes`, read from `offset`, to a [`BlobRange`]: leading continuation bytes are skipped
/// (moving `offset` past them), then the whole characters that end within `len` bytes and within
/// `bytes` are kept, or the first character when none does. A character the read cut short is
/// left for the next read, which starts at `end`. Bytes that are not UTF-8 are a corrupt store.
pub(crate) fn cut(offset: Count, len: ReadLen, bytes: &[u8]) -> Result<BlobRange, Error> {
    let skip = bytes.iter().take_while(|b| (**b & 0xC0) == 0x80).count();
    let rest = bytes.get(skip..).unwrap_or_default();
    let valid = match std::str::from_utf8(rest) {
        Ok(text) => text,
        // An incomplete character at the end is where the read stopped, not corruption.
        Err(error) if error.error_len().is_none() => {
            let (head, _) = rest
                .split_at_checked(error.valid_up_to())
                .unwrap_or_default();
            std::str::from_utf8(head).map_err(|e| corrupt(&e))?
        }
        Err(error) => return Err(corrupt(&error)),
    };
    let limit = usize::try_from(len.get()).unwrap_or(usize::MAX);
    let mut end = limit.min(valid.len());
    while !valid.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    if end == 0 {
        end = valid.chars().next().map_or(0, char::len_utf8);
    }
    let (text, _) = valid.split_at_checked(end).unwrap_or_default();
    let start = add(offset, skip)?;
    Ok(BlobRange {
        offset: start,
        end: add(start, text.len())?,
        text: text.to_owned(),
    })
}

fn add(count: Count, bytes: usize) -> Result<Count, Error> {
    let bytes = u64::try_from(bytes).map_err(|e| Error::Internal(e.to_string()))?;
    Ok(Count::new(count.get().saturating_add(bytes))?)
}

fn corrupt(error: &std::str::Utf8Error) -> Error {
    Error::Store(clauseo_store::Error::Corrupt {
        detail: format!("a blob is not UTF-8 text: {error}"),
    })
}

#[cfg(test)]
mod tests;
