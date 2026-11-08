//! A pack's content hash over every file of the pack, its manifest included, written
//! `sha256:<hex>` so a later algorithm can be told apart.
//!
//! The SHA-256 of one stream, `clauseo-pack-hash-v1` ([`DOMAIN`]):
//!
//! ```text
//! "clauseo-pack-hash-v1" 0x00
//! for each file, sorted by its path relative to the pack (`/`-separated, compared as bytes):
//!     len(path) as a little-endian u64, then the path's bytes
//!     len(bytes) as a little-endian u64, then the file's bytes
//! ```
//!
//! Every field carries its length, so no file's bytes (which may hold a NUL) can stand for a
//! file boundary, and two different packs never feed the same stream. It is not 0031's
//! `versions.lock` algorithm, which hashes only Clauseo's own files and stays as it is.
//! `sha2` picks the CPU's SHA instructions at run time where it has them (the `SHA-NI`
//! extension on `x86-64`, the `ARMv8` SHA extension on `aarch64`).

use std::fmt;
use std::fmt::Write as _;

use sha2::{Digest as _, Sha256};

/// The prefix of every hash.
const PREFIX: &str = "sha256:";

/// The domain tag the stream starts with, its NUL included.
pub const DOMAIN: &[u8] = b"clauseo-pack-hash-v1\0";

/// The hex digits of a SHA-256.
const HEX_LEN: usize = 64;

/// A pack's content hash, `sha256:` and 64 lower-case hex digits. Only [`content_hash`] and
/// [`PackHash::parse`] make one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PackHash(String);

impl PackHash {
    /// `text` when it is `sha256:` and 64 lower-case hex digits.
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.strip_prefix(PREFIX)?;
        let valid = hex.len() == HEX_LEN
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        valid.then(|| Self(text.to_owned()))
    }

    /// The hash as written: `sha256:<hex>`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The 64 hex digits alone, a name any file system accepts.
    pub fn hex(&self) -> &str {
        self.0.get(PREFIX.len()..).unwrap_or_default()
    }
}

impl fmt::Display for PackHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The hash of `files`, each a path relative to the pack and its bytes, in any order.
pub fn content_hash<'a>(files: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> PackHash {
    let mut sorted: Vec<(&str, &[u8])> = files.into_iter().collect();
    sorted.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    for (path, bytes) in sorted {
        for field in [path.as_bytes(), bytes] {
            hasher.update((field.len() as u64).to_le_bytes());
            hasher.update(field);
        }
    }
    let mut out = String::with_capacity(PREFIX.len() + HEX_LEN);
    out.push_str(PREFIX);
    for byte in hasher.finalize() {
        let _infallible = write!(out, "{byte:02x}");
    }
    PackHash(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_parses_only_in_its_form() {
        let hash = content_hash([("a", b"x".as_slice())]);
        assert_eq!(PackHash::parse(hash.as_str()), Some(hash.clone()));
        assert_eq!(hash.hex().len(), 64);
        for bad in ["", "sha256:", "sha1:00", &hash.as_str().to_uppercase()] {
            assert_eq!(PackHash::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_stream_is_the_tag_then_each_length_and_field() {
        let mut stream = b"clauseo-pack-hash-v1\0".to_vec();
        for (path, bytes) in [("a.md", b"x\0y".as_slice()), ("b/c.md", b"".as_slice())] {
            for field in [path.as_bytes(), bytes] {
                stream.extend(u64::try_from(field.len()).unwrap().to_le_bytes());
                stream.extend(field);
            }
        }
        let expected = Sha256::digest(&stream)
            .iter()
            .fold(String::new(), |mut hex, byte| {
                let _infallible = write!(hex, "{byte:02x}");
                hex
            });
        let hash = content_hash([("b/c.md", b"".as_slice()), ("a.md", b"x\0y".as_slice())]);
        assert_eq!(hash.as_str(), format!("sha256:{expected}"));
        // The same bytes split at another file boundary hash apart.
        let moved = content_hash([("a.md", b"x".as_slice()), ("b/c.md", b"\0y".as_slice())]);
        assert_ne!(moved, hash);
    }

    #[test]
    fn the_order_given_does_not_matter() {
        let one = content_hash([("b", b"2".as_slice()), ("a", b"1".as_slice())]);
        let two = content_hash([("a", b"1".as_slice()), ("b", b"2".as_slice())]);
        assert_eq!(one, two);
    }
}
