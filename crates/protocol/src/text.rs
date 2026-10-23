//! Text costs and large payloads: what one view entry may carry inline, and how it is counted.
//!
//! A string's cost is its UTF-8 bytes after JSON escaping, without the quotes, as `serde_json`
//! writes it. A view entry's cost is that, summed over the `Inline` values among its
//! [`TextOrBlob`] fields, `Vec` items included. The count is [`escaped_len`] and
//! [`TextOrBlob::inline_len`], and nothing else counts a view entry's inline text.

use serde::{Deserialize, Serialize};

use crate::{BlobHash, Count, Error};

/// The most one view entry's inline `TextOrBlob` values may cost together, counted by
/// [`escaped_len`]. Larger payloads go to a blob.
pub const INLINE_LIMIT_BYTES: usize = 4096;

/// The most UTF-8 bytes a blob's preview holds.
pub const PREVIEW_BYTES: usize = 512;

/// Extra bytes `serde_json` adds when it escapes each byte value: 1 for `"`, `\` and the five
/// control characters with a short escape (`\b`, `\t`, `\n`, `\f`, `\r`), 5 for every other byte
/// below 0x20 (`\u00XX`), 0 for the rest (`serde_json` leaves DEL and non-ASCII as they are).
static ESCAPE_EXTRA: [u8; 256] = escape_table();

const fn escape_table() -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut rest: &mut [u8] = &mut table;
    let mut byte: u8 = 0;
    while let [slot, tail @ ..] = rest {
        *slot = match byte {
            0x08 | 0x09 | 0x0A | 0x0C | 0x0D | b'"' | b'\\' => 1,
            0x00..=0x1F => 5,
            _ => 0,
        };
        rest = tail;
        byte = byte.wrapping_add(1);
    }
    table
}

fn extra(byte: u8) -> usize {
    // A u8 index is always in bounds, so the optimiser drops the `None` arm.
    usize::from(ESCAPE_EXTRA.get(usize::from(byte)).copied().unwrap_or(0))
}

/// The cost of `text`: its UTF-8 bytes after JSON escaping, without the quotes. Never allocates.
///
/// It equals `serde_json::to_string(text).len() - 2`; a proptest checks that.
pub fn escaped_len(text: &str) -> usize {
    // A word-at-a-time scan: typical text has escapes far apart, so it skips 8 clean bytes per
    // step and looks bytes up in the table only in a word that has an escape.
    escaped_len_swar(text)
}

const ONES: u64 = u64::from_ne_bytes([0x01; 8]);
const HIGHS: u64 = u64::from_ne_bytes([0x80; 8]);

/// True when some byte of `word` is zero (exact, no false positive).
const fn has_zero_byte(word: u64) -> bool {
    word.wrapping_sub(ONES) & !word & HIGHS != 0
}

/// True when some byte of `word` needs escaping: below 0x20, `"` or `\`.
const fn needs_escape(word: u64) -> bool {
    // The classic "has a byte less than n" test, exact for n <= 128.
    let below_space = word.wrapping_sub(ONES * 0x20) & !word & HIGHS != 0;
    below_space || has_zero_byte(word ^ (ONES * 0x22)) || has_zero_byte(word ^ (ONES * 0x5C))
}

/// The scanning variant: tests 8 bytes at a time and uses the table only on a hit.
fn escaped_len_swar(text: &str) -> usize {
    let (words, tail) = text.as_bytes().as_chunks::<8>();
    let mut extra_bytes = 0;
    for word in words {
        if needs_escape(u64::from_ne_bytes(*word)) {
            extra_bytes += word.iter().copied().map(extra).sum::<usize>();
        }
    }
    extra_bytes += tail.iter().copied().map(extra).sum::<usize>();
    text.len() + extra_bytes
}

/// The length of `text` in UTF-16 code units, which is what JavaScript's `string.length` counts.
///
/// Computed as the number of chars plus the number of bytes at or above 0xF0 (each starts a
/// 4-byte character, which takes a surrogate pair), never by encoding to UTF-16.
pub fn utf16_len(text: &str) -> usize {
    text.chars().count() + text.bytes().filter(|&byte| byte >= 0xF0).count()
}

/// The longest prefix of `text` that fits in [`PREVIEW_BYTES`] and ends on a char boundary.
/// It computes the preview of a text; a [`BlobRef`] returns the one it stores, from
/// [`BlobRef::preview`].
pub fn preview_of(text: &str) -> &str {
    let end = text.floor_char_boundary(PREVIEW_BYTES);
    text.get(..end).unwrap_or_default()
}

/// A payload carried inline, or stored as a blob with a preview.
///
/// JSON: `{"inline": "<text>"}` or `{"blob": {"hash": "<hex>", "size_bytes": 3, "preview": ".."}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum TextOrBlob {
    /// The text itself.
    Inline(String),
    /// A reference to a blob holding the text.
    Blob(BlobRef),
}

impl TextOrBlob {
    /// This value's share of an entry's inline cost: [`escaped_len`] of an `Inline` text, 0 for
    /// a `Blob` (its preview does not count).
    pub fn inline_len(&self) -> usize {
        match self {
            Self::Inline(text) => escaped_len(text),
            Self::Blob(_) => 0,
        }
    }
}

/// A blob: its BLAKE3 hash, its size and the first bytes of its text.
///
/// Its fields are private so every value passed [`BlobRef::new`]'s checks; `Deserialize` runs
/// the same checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BlobRefWire")]
// specta: `try_from` only adds checks; the TypeScript shape is the struct's own fields.
#[cfg_attr(feature = "specta", derive(specta::Type), specta(skip_attr = "serde"))]
pub struct BlobRef {
    hash: BlobHash,
    size_bytes: Count,
    preview: String,
}

impl BlobRef {
    /// Builds a reference, checking that `preview` is at most [`PREVIEW_BYTES`] and no longer
    /// than the blob it starts, `size_bytes`. `Deserialize` applies the same checks.
    pub fn new(hash: BlobHash, size_bytes: Count, preview: String) -> Result<Self, Error> {
        if preview.len() > PREVIEW_BYTES {
            return Err(Error::Format {
                kind: "BlobRef.preview",
                rule: "longer than 512 bytes",
            });
        }
        if preview.len() as u64 > size_bytes.get() {
            return Err(Error::Format {
                kind: "BlobRef.preview",
                rule: "longer than the blob's size_bytes",
            });
        }
        Ok(Self {
            hash,
            size_bytes,
            preview,
        })
    }

    /// The blob's BLAKE3 digest.
    pub const fn hash(&self) -> BlobHash {
        self.hash
    }

    /// The blob's size in bytes.
    pub const fn size_bytes(&self) -> Count {
        self.size_bytes
    }

    /// The start of the text, at most [`PREVIEW_BYTES`] (see [`preview_of`]).
    pub fn preview(&self) -> &str {
        &self.preview
    }
}

/// `BlobRef` as read, before its checks.
#[derive(Deserialize)]
struct BlobRefWire {
    hash: BlobHash,
    size_bytes: Count,
    preview: String,
}

impl TryFrom<BlobRefWire> for BlobRef {
    type Error = Error;

    fn try_from(wire: BlobRefWire) -> Result<Self, Error> {
        Self::new(wire.hash, wire.size_bytes, wire.preview)
    }
}

#[cfg(test)]
mod tests;
