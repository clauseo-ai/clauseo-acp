//! The digests downloads are checked against: a package's SHA-512 from the lock's `integrity`,
//! and a Node archive's SHA-256 from the snapshot.

use std::fmt;

use base64::Engine as _;
use sha2::{Digest as _, Sha256, Sha512};

/// The digest a downloaded file must have.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum Expected {
    /// SHA-512, from an npm lock's `integrity` (`sha512-<base64>`).
    Sha512([u8; 64]),
    /// SHA-256, from Node's `SHASUMS256.txt`.
    Sha256([u8; 32]),
}

/// A lock `integrity` that is not one `sha512-<base64>` of 64 bytes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the integrity {0:?} is not one sha512-<base64> digest")]
pub(crate) struct BadIntegrity(pub(crate) String);

impl Expected {
    /// Reads an npm `integrity` string. Only one `sha512-` entry is accepted: every package in
    /// the pins has exactly that, and a weaker or mixed list is refused rather than trusted.
    pub(crate) fn from_integrity(integrity: &str) -> Result<Expected, BadIntegrity> {
        let bad = || BadIntegrity(integrity.to_owned());
        let encoded = integrity.trim().strip_prefix("sha512-").ok_or_else(bad)?;
        if encoded.contains(char::is_whitespace) {
            return Err(bad());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_not_base64| bad())?;
        let digest: [u8; 64] = bytes.try_into().map_err(|_wrong_length| bad())?;
        Ok(Expected::Sha512(digest))
    }

    /// Reads 64 hex digits of a SHA-256.
    pub(crate) fn sha256_hex(hex: &str) -> Option<Expected> {
        let bytes = from_hex(hex)?;
        Some(Expected::Sha256(bytes.try_into().ok()?))
    }

    /// The digest in lowercase hex: a downloaded file's name (`<hex>.tgz`), and an offline
    /// bundle's file name for the same package.
    pub(crate) fn hex(&self) -> String {
        match self {
            Expected::Sha512(bytes) => to_hex(bytes),
            Expected::Sha256(bytes) => to_hex(bytes),
        }
    }

    /// A hasher of the same kind.
    pub(crate) fn hasher(&self) -> Hasher {
        match self {
            Expected::Sha512(_) => Hasher::Sha512(Sha512::new()),
            Expected::Sha256(_) => Hasher::Sha256(Sha256::new()),
        }
    }
}

impl fmt::Debug for Expected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expected::Sha512(_) => write!(f, "sha512:{}", self.hex()),
            Expected::Sha256(_) => write!(f, "sha256:{}", self.hex()),
        }
    }
}

/// A running SHA-512 or SHA-256.
#[derive(Clone)]
pub(crate) enum Hasher {
    /// SHA-512.
    Sha512(Sha512),
    /// SHA-256.
    Sha256(Sha256),
}

impl Hasher {
    /// Adds `bytes`.
    pub(crate) fn update(&mut self, bytes: &[u8]) {
        match self {
            Hasher::Sha512(hasher) => hasher.update(bytes),
            Hasher::Sha256(hasher) => hasher.update(bytes),
        }
    }

    /// Whether everything added has the digest `expected`.
    pub(crate) fn matches(self, expected: &Expected) -> bool {
        match (self, expected) {
            (Hasher::Sha512(hasher), Expected::Sha512(want)) => hasher.finalize()[..] == want[..],
            (Hasher::Sha256(hasher), Expected::Sha256(want)) => hasher.finalize()[..] == want[..],
            _ => false,
        }
    }
}

impl fmt::Debug for Hasher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Hasher::Sha512(_) => "Hasher::Sha512",
            Hasher::Sha256(_) => "Hasher::Sha256",
        })
    }
}

/// The first 16 hex digits of the SHA-256 of `parts`, each prefixed by its length so no two
/// different lists hash alike: the install folder's `<hash>`.
pub(crate) fn short_hash(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(u64::try_from(part.len()).unwrap_or(u64::MAX).to_le_bytes());
        hasher.update(part);
    }
    let mut hex = to_hex(&hasher.finalize());
    hex.truncate(16);
    hex
}

/// Lowercase hex of `bytes`.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// Bytes from hex digits of either case, or `None` for an odd length or a non-hex digit.
fn from_hex(hex: &str) -> Option<Vec<u8>> {
    let digit = |byte: u8| {
        char::from(byte)
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
    };
    let bytes = hex.as_bytes();
    let (pairs, rest) = bytes.as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    pairs
        .iter()
        .map(|&[high, low]| Some(digit(high)? << 4 | digit(low)?))
        .collect()
}

#[cfg(test)]
mod tests;
