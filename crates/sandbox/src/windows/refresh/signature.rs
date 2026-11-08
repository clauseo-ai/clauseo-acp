//! The staged exe's signature check (ADR 0049, "Admin copy refresh"): the `.sig` the Tauri
//! updater writes beside the exe, checked against the updater key built in (`key.rs`,
//! `build.rs`). Pure: bytes in, a verdict out, on every OS.
//!
//! The `.sig` is Tauri's form of a minisign signature: base64 of the whole signature file (an
//! untrusted comment, the signature line, a trusted comment, the global signature). Only a
//! prehashed signature (`ED`, BLAKE2b-512 of the file signed) is taken; the legacy form (`Ed`,
//! the file itself signed) is refused. Once the signature and the global signature over the
//! trusted comment both verify, the trusted comment is read as the Tauri CLI writes it
//! (tauri-cli 2.12, `updater_signature.rs`): `timestamp:<seconds>\tfile:<basename>\tversion:<v>`,
//! tab-separated `key:value` fields, matched by key in any order. A field with no `:` or a key
//! given twice is refused. `file` must be exactly `clauseo-sandbox-setup.exe`, so a signed
//! installer or any other artefact signed with the same key can't stand in for it. `version`
//! must be there; it comes back to the caller, which requires the staged bytes' own
//! `FILEVERSION` to be its core (`version.rs`) and orders it against the record, so the
//! signature binds the version the downgrade check uses. `timestamp` is informational.

use std::collections::HashMap;

use minisign_verify::{Error as MinisignError, PublicKey, Signature};

use super::key::{PLACEHOLDER_KEY_ID, compact, decode_base64, key_id};

/// The file name the trusted comment must name.
pub const SIGNED_FILE: &str = "clauseo-sandbox-setup.exe";

/// Why a staged exe's signature was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    /// The key built in is the placeholder: a build given no updater key verifies nothing.
    #[error("the updater key built into this exe is the placeholder, which verifies nothing")]
    PlaceholderKey,
    /// The key built in isn't a minisign public key (`build.rs` checks it, so a bad build).
    #[error("the updater key built into this exe isn't a minisign public key: {0}")]
    BadKey(String),
    /// The `.sig` isn't Tauri's base64 of a minisign signature file.
    #[error("the signature file is malformed: {0}")]
    Malformed(&'static str),
    /// The signature names another key.
    #[error("the signature was made with another key")]
    WrongKey,
    /// A legacy signature, of the file itself rather than its BLAKE2b-512 hash.
    #[error("the signature isn't a prehashed (ED) signature")]
    NotPrehashed,
    /// The signature, or the global signature over the trusted comment, doesn't verify.
    #[error("the signature doesn't verify")]
    Invalid,
    /// The trusted comment names another file, or none.
    #[error("the signature is for another file than the one checked")]
    WrongFile,
    /// The trusted comment has a field with no `:`, or a key given twice.
    #[error("the signature's trusted comment is malformed: {0}")]
    BadComment(&'static str),
    /// The trusted comment carries no `version`, or an empty one.
    #[error("the signature carries no version")]
    NoVersion,
    /// The staged exe's `FILEVERSION` isn't the signed `version`'s core (the caller's check).
    #[error("the signature is for version {signed}, but the exe's FILEVERSION is {staged}")]
    VersionMismatch {
        /// The trusted comment's `version`.
        signed: String,
        /// The staged exe's `FILEVERSION`.
        staged: String,
    },
}

/// What a verified signature vouches for, beyond the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    /// The trusted comment's `version`, as written.
    pub version: String,
}

/// Checks that `sig_file` (the `.sig`'s bytes) is a valid signature of `exe` by `key` (Tauri's
/// base64 of a minisign public key), for [`SIGNED_FILE`], and returns its signed version.
pub fn verify(exe: &[u8], sig_file: &[u8], key: &str) -> Result<Signed, SignatureError> {
    verify_file(exe, sig_file, key, SIGNED_FILE)
}

/// [`verify`] for the file named `file` in the trusted comment: the Git Bash stub's source,
/// `clauseo-sandbox-run.exe`, is held to the same key and rules (`git_tree::stub`).
pub fn verify_file(
    exe: &[u8],
    sig_file: &[u8],
    key: &str,
    file: &str,
) -> Result<Signed, SignatureError> {
    let id = key_id(key).map_err(SignatureError::BadKey)?;
    if id == PLACEHOLDER_KEY_ID {
        return Err(SignatureError::PlaceholderKey);
    }
    let key = untauri(key.as_bytes()).ok_or(SignatureError::BadKey("not base64".to_owned()))?;
    let key = PublicKey::decode(&key).map_err(|error| SignatureError::BadKey(error.to_string()))?;
    let signature = untauri(sig_file).ok_or(SignatureError::Malformed("not base64 text"))?;
    let signature = Signature::decode(&signature).map_err(|error| match error {
        MinisignError::UnsupportedAlgorithm => SignatureError::Malformed("unknown algorithm"),
        MinisignError::InvalidEncoding
        | MinisignError::InvalidSignature
        | MinisignError::IoError(_)
        | MinisignError::UnexpectedAlgorithm
        | MinisignError::UnexpectedKeyId
        | MinisignError::UnsupportedLegacyMode => {
            SignatureError::Malformed("not a minisign signature")
        }
    })?;
    key.verify(exe, &signature, false)
        .map_err(|error| match error {
            MinisignError::UnexpectedKeyId => SignatureError::WrongKey,
            MinisignError::UnexpectedAlgorithm => SignatureError::NotPrehashed,
            MinisignError::InvalidEncoding
            | MinisignError::InvalidSignature
            | MinisignError::IoError(_)
            | MinisignError::UnsupportedAlgorithm
            | MinisignError::UnsupportedLegacyMode => SignatureError::Invalid,
        })?;
    // Only now is the trusted comment the signer's: the global signature verified above covers
    // it, and nothing before this line reads it.
    let fields = fields(signature.trusted_comment())?;
    if fields.get("file") != Some(&file) {
        return Err(SignatureError::WrongFile);
    }
    match fields.get("version") {
        Some(version) if !version.is_empty() => Ok(Signed {
            version: (*version).to_owned(),
        }),
        _ => Err(SignatureError::NoVersion),
    }
}

/// The trusted comment's tab-separated `key:value` fields, by key (the value is everything
/// after the first `:`). An empty comment has none; a field with no `:` or a key given twice
/// is refused.
fn fields(comment: &str) -> Result<HashMap<&str, &str>, SignatureError> {
    let mut fields = HashMap::new();
    if comment.is_empty() {
        return Ok(fields);
    }
    for field in comment.split('\t') {
        let (key, value) = field
            .split_once(':')
            .ok_or(SignatureError::BadComment("a field has no ':'"))?;
        if fields.insert(key, value).is_some() {
            return Err(SignatureError::BadComment("a field is given twice"));
        }
    }
    Ok(fields)
}

/// Tauri's base64 wrapping of a minisign file undone: the text inside, or `None` when the
/// bytes aren't base64 of UTF-8 text.
fn untauri(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    String::from_utf8(decode_base64(&compact(text))?).ok()
}

#[cfg(test)]
mod tests;
