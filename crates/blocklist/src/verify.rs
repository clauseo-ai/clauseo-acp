//! The minisign check: the signature over the exact `.gz` bytes, by one of the compiled-in
//! keys, and the trusted comment it covers.

use minisign_verify::{PublicKey, Signature};

use crate::keys::key_line;
use crate::list::BlockList;
use crate::parse::{MAX_ENTRIES, parse_gz};
use crate::{Error, Generated};

/// The largest `.minisig` file read: a real one is about 330 bytes.
pub const MAX_SIGNATURE: usize = 4096;

/// What the trusted comment says, once the signature verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// When the list was made.
    pub generated: Generated,
    /// How many entry lines it has.
    pub entries: u32,
}

/// Checks `minisig` over `gz` against `keys` (minisign public key files or bare key lines;
/// entries with no usable key line are skipped) and reads its trusted comment. Only prehashed
/// signatures (`ED`, minisign's default) verify; the legacy kind is refused.
pub fn verify(gz: &[u8], minisig: &[u8], keys: &[&str]) -> Result<Header, Error> {
    if minisig.len() > MAX_SIGNATURE {
        return Err(Error::SignatureFile);
    }
    let text = std::str::from_utf8(minisig)
        .ok()
        .ok_or(Error::SignatureFile)?;
    let signature = Signature::decode(text).ok().ok_or(Error::SignatureFile)?;
    let keys: Vec<PublicKey> = keys
        .iter()
        .filter_map(|key| PublicKey::from_base64(key_line(key)?).ok())
        .collect();
    if keys.is_empty() {
        return Err(Error::NoKey);
    }
    if !keys
        .iter()
        .any(|key| key.verify(gz, &signature, false).is_ok())
    {
        return Err(Error::Signature);
    }
    header(signature.trusted_comment()).ok_or(Error::Comment)
}

/// Verifies, then decompresses and parses: the list, or why it was refused.
pub fn load(gz: &[u8], minisig: &[u8], keys: &[&str]) -> Result<BlockList, Error> {
    let header = verify(gz, minisig, keys)?;
    parse_gz(gz, header)
}

/// `clauseo-blocklist v1 generated=<time> entries=<n>`, exactly: single spaces, `n` in decimal
/// without a sign or leading zeros.
fn header(comment: &str) -> Option<Header> {
    let rest = comment.strip_prefix("clauseo-blocklist v1 generated=")?;
    let (generated, entries) = rest.split_once(" entries=")?;
    let generated = Generated::parse(generated)?;
    let digits = !entries.is_empty() && entries.bytes().all(|b| b.is_ascii_digit());
    if !digits || (entries.len() > 1 && entries.starts_with('0')) || entries.len() > 10 {
        return None;
    }
    let entries: u64 = entries.parse().ok()?;
    // Over the bound is a list too big, not a malformed comment: say so.
    let entries = u32::try_from(entries.min(u64::from(MAX_ENTRIES) + 1)).ok()?;
    Some(Header { generated, entries })
}
