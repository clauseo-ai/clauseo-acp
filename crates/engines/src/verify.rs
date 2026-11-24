//! Tauri's updater signature, what `tauri signer sign` writes to `<file>.sig`: base64 of a
//! minisign signature file (an untrusted comment line, the signature line, `trusted comment:
//! timestamp:<n>\tfile:<name>` and the global signature line). Checked over the exact
//! `engines.json` bytes with the key, then the trusted comment, which the global signature
//! covers, must name `engines.json` and nothing more.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use minisign_verify::{PublicKey, Signature};

use crate::keys::compact;
use crate::{Error, LIST_FILE, MAX_SIGNATURE};

/// Checks `sig` over `json` against `key` (Tauri's base64 form of a minisign public key file).
/// Only prehashed signatures (`ED`, what Tauri writes) verify.
pub(crate) fn verify(json: &[u8], sig: &[u8], key: &str) -> Result<(), Error> {
    let key = public_key(key).ok_or(Error::NoKey)?;
    if sig.len() > MAX_SIGNATURE {
        return Err(Error::SignatureFile);
    }
    let sig = std::str::from_utf8(sig).ok().ok_or(Error::SignatureFile)?;
    let file = STANDARD
        .decode(compact(sig))
        .ok()
        .ok_or(Error::SignatureFile)?;
    let file = String::from_utf8(file).ok().ok_or(Error::SignatureFile)?;
    let signature = Signature::decode(&file).ok().ok_or(Error::SignatureFile)?;
    key.verify(json, &signature, false)
        .ok()
        .ok_or(Error::Signature)?;
    if comment_is_exact(signature.trusted_comment()) {
        Ok(())
    } else {
        Err(Error::Comment)
    }
}

/// The key in Tauri's form: base64 of a `.pub` file whose second line is the `RW…` key line.
fn public_key(key: &str) -> Option<PublicKey> {
    let file = STANDARD.decode(compact(key)).ok()?;
    let file = String::from_utf8(file).ok()?;
    PublicKey::decode(&file).ok()
}

/// `timestamp:<one or more ASCII digits>\tfile:engines.json`, exactly.
fn comment_is_exact(comment: &str) -> bool {
    let Some(rest) = comment.strip_prefix("timestamp:") else {
        return false;
    };
    let Some((digits, file)) = rest.split_once('\t') else {
        return false;
    };
    !digits.is_empty()
        && digits.len() <= 20
        && digits.bytes().all(|b| b.is_ascii_digit())
        && file.strip_prefix("file:") == Some(LIST_FILE)
}
