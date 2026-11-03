//! [`Cursor`]: where the next page starts, and its text form for the app.
//!
//! The text is base64url (no padding) of a small versioned record: version 1, the query's check
//! (FNV-1a 64 of what the query asks, stable across Rust releases), then the position. Decoding
//! refuses anything else; [`SearchIndex::query`](crate::SearchIndex::query) starts over from the
//! first page when a cursor does not belong to the query it comes with.

use crate::query::{Mode, Query, WINDOW};
use crate::{Error, Kind};

const VERSION: u8 = 1;
const TAG_BEFORE: u8 = 0;
const TAG_RANKED: u8 = 1;

/// Where the next page starts. It belongs to one query: used with another text, scope, kinds,
/// archived choice or mode, the query starts over from the first page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub(crate) check: u64,
    pub(crate) at: At,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum At {
    /// Newest first: rows older than this rowid.
    Before(i64),
    /// Ranked: the window of rows up to `top` ranked at `now`, from `offset` (at most
    /// [`WINDOW`]).
    Ranked { offset: u32, top: i64, now: i64 },
}

impl Cursor {
    /// The cursor as text: base64url without padding, safe in JSON and URLs.
    pub fn encode(&self) -> String {
        let mut bytes = vec![VERSION];
        bytes.extend_from_slice(&self.check.to_le_bytes());
        match self.at {
            At::Before(rowid) => {
                bytes.push(TAG_BEFORE);
                bytes.extend_from_slice(&rowid.to_le_bytes());
            }
            At::Ranked { offset, top, now } => {
                bytes.push(TAG_RANKED);
                bytes.extend_from_slice(&offset.to_le_bytes());
                bytes.extend_from_slice(&top.to_le_bytes());
                bytes.extend_from_slice(&now.to_le_bytes());
            }
        }
        base64url(&bytes)
    }

    /// Reads [`Cursor::encode`]'s text. Anything else, or a position no page can have, is
    /// [`Error::Cursor`].
    pub fn decode(text: &str) -> Result<Self, Error> {
        let bytes = unbase64url(text).ok_or(Error::Cursor)?;
        let (&version, rest) = bytes.split_first().ok_or(Error::Cursor)?;
        if version != VERSION {
            return Err(Error::Cursor);
        }
        let (check, rest) = take::<8>(rest)?;
        let (&tag, rest) = rest.split_first().ok_or(Error::Cursor)?;
        let at = match (tag, rest.len()) {
            (TAG_BEFORE, 8) => {
                let (rowid, _) = take::<8>(rest)?;
                At::Before(i64::from_le_bytes(rowid))
            }
            (TAG_RANKED, 20) => {
                let (offset, rest) = take::<4>(rest)?;
                let (top, rest) = take::<8>(rest)?;
                let (now, _) = take::<8>(rest)?;
                At::Ranked {
                    offset: u32::from_le_bytes(offset),
                    top: i64::from_le_bytes(top),
                    now: i64::from_le_bytes(now),
                }
            }
            _ => return Err(Error::Cursor),
        };
        let cursor = Self {
            check: u64::from_le_bytes(check),
            at,
        };
        if cursor.valid() {
            Ok(cursor)
        } else {
            Err(Error::Cursor)
        }
    }

    /// Whether the position is one a page can have.
    pub(crate) fn valid(&self) -> bool {
        match self.at {
            At::Before(rowid) => rowid > 0,
            At::Ranked { offset, top, .. } => offset <= WINDOW && top >= 0,
        }
    }
}

fn take<const N: usize>(bytes: &[u8]) -> Result<([u8; N], &[u8]), Error> {
    let (head, rest) = bytes.split_at_checked(N).ok_or(Error::Cursor)?;
    let head = <[u8; N]>::try_from(head).ok().ok_or(Error::Cursor)?;
    Ok((head, rest))
}

/// What a cursor belongs to: everything in the query but the cursor, the limit and the clock,
/// as FNV-1a 64 (written out, so it never changes with a Rust release).
pub(crate) fn check(query: &Query) -> u64 {
    let mut hash = Fnv::new();
    hash.bytes(
        &u64::try_from(query.text.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    hash.bytes(query.text.as_bytes());
    match query.matter {
        None => hash.bytes(&[0]),
        Some(matter) => {
            hash.bytes(&[1]);
            hash.bytes(matter.as_uuid().as_bytes());
        }
    }
    match &query.kinds {
        None => hash.bytes(&[0]),
        Some(kinds) => {
            hash.bytes(&[1]);
            hash.bytes(&u64::try_from(kinds.len()).unwrap_or(u64::MAX).to_le_bytes());
            for kind in kinds {
                hash.bytes(&[kind_byte(*kind)]);
            }
        }
    }
    hash.bytes(&[u8::from(query.include_archived)]);
    hash.bytes(&[match query.mode {
        Mode::Typing => 0,
        Mode::Ranked => 1,
    }]);
    hash.0
}

fn kind_byte(kind: Kind) -> u8 {
    u8::try_from(kind.code()).unwrap_or(u8::MAX)
}

struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut word = [0u8; 3];
        word[..chunk.len()].copy_from_slice(chunk);
        let n = u32::from(word[0]) << 16 | u32::from(word[1]) << 8 | u32::from(word[2]);
        for i in 0..=chunk.len() {
            let index = (n >> (18 - 6 * i)) & 63;
            out.push(char::from(ALPHABET[usize::try_from(index).unwrap_or(0)]));
        }
    }
    out
}

fn unbase64url(text: &str) -> Option<Vec<u8>> {
    if text.len() % 4 == 1 || text.len() > 64 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    for chunk in text.as_bytes().chunks(4) {
        let mut n = 0u32;
        for (i, byte) in chunk.iter().enumerate() {
            let value = ALPHABET.iter().position(|a| a == byte)?;
            n |= u32::try_from(value).ok()? << (18 - 6 * i);
        }
        let [_, a, b, c] = n.to_be_bytes();
        out.extend_from_slice(&[a, b, c][..chunk.len() - 1]);
    }
    // Only the canonical text decodes: re-encoding must give it back.
    (base64url(&out) == text).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::{At, Cursor, base64url, unbase64url};
    use crate::Error;

    #[test]
    fn base64url_round_trips_every_length() {
        for len in 0..40u8 {
            let bytes: Vec<u8> = (0..len).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(unbase64url(&base64url(&bytes)), Some(bytes));
        }
        assert_eq!(base64url(b"\xfb\xff"), "-_8");
    }

    #[test]
    fn a_cursor_round_trips_through_text() {
        for at in [
            At::Before(42),
            At::Ranked {
                offset: 40,
                top: 105_200,
                now: 1_730_000_000_000,
            },
        ] {
            let cursor = Cursor {
                check: 0xdead_beef,
                at,
            };
            assert_eq!(Cursor::decode(&cursor.encode()).ok(), Some(cursor));
        }
    }

    #[test]
    fn garbled_or_impossible_text_is_an_error() {
        let past_window = Cursor {
            check: 1,
            at: At::Ranked {
                offset: u32::MAX,
                top: 1,
                now: 0,
            },
        };
        let negative = Cursor {
            check: 1,
            at: At::Before(-5),
        };
        let good = Cursor {
            check: 1,
            at: At::Before(5),
        }
        .encode();
        let inputs = [
            String::new(),
            "!!!!".to_owned(),
            "A".to_owned(),
            "A".repeat(1_000),
            good[..good.len() - 1].to_owned(),
            format!("{good}A"),
            format!("{good}="),
            past_window.encode(),
            negative.encode(),
            base64url(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0]),
            base64url(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 9, 5, 0, 0, 0, 0, 0, 0, 0]),
        ];
        for text in inputs {
            assert!(
                matches!(Cursor::decode(&text), Err(Error::Cursor)),
                "{text:?}"
            );
        }
    }
}
