//! The staged exe's signature check (ADR 0049, "Admin copy refresh"): the `.sig` the Tauri
//! updater writes beside the exe, checked against the updater key built in (`key.rs`,
//! `build.rs`). Pure: bytes in, a verdict out, on every OS.
//!
//! The `.sig` is Tauri's form of a minisign signature: base64 of the whole signature file (an
//! untrusted comment, the signature line, a trusted comment, the global signature). Only a
//! prehashed signature (`ED`, BLAKE2b-512 of the file signed) is taken; the legacy form (`Ed`,
//! the file itself signed) is refused. Once the signature and the global signature over the
//! trusted comment both verify, the trusted comment, which the Tauri CLI writes as tab-separated
//! `key:value` pairs, must name the file `clauseo-sandbox-setup.exe` (so a signed installer or
//! any other artefact signed with the same key can't stand in for it) and carry a `version:` no
//! lower than this exe's own (so an older signed exe can't roll the admin copy back).

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
    #[error("the signature is for another file than {SIGNED_FILE}")]
    WrongFile,
    /// The trusted comment carries no version, or one that isn't `major.minor.patch`.
    #[error("the signature carries no readable version")]
    NoVersion,
    /// The signed version is lower than this exe's.
    #[error("the signed version {signed} is older than this exe's {own}")]
    Downgrade {
        /// The trusted comment's version.
        signed: String,
        /// This exe's version.
        own: String,
    },
}

/// Checks that `sig_file` (the `.sig`'s bytes) is a valid signature of `exe` by `key` (Tauri's
/// base64 of a minisign public key), for [`SIGNED_FILE`] at a version no lower than `own`.
pub fn verify(exe: &[u8], sig_file: &[u8], key: &str, own: &str) -> Result<(), SignatureError> {
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
    trusted(signature.trusted_comment(), own)
}

/// The trusted comment's checks: [`SIGNED_FILE`], and a version no lower than `own`.
fn trusted(comment: &str, own: &str) -> Result<(), SignatureError> {
    let field = |name: &str| comment.split('\t').find_map(|pair| pair.strip_prefix(name));
    if field("file:") != Some(SIGNED_FILE) {
        return Err(SignatureError::WrongFile);
    }
    let signed = field("version:").ok_or(SignatureError::NoVersion)?;
    let (Some(signed_core), Some(own_core)) = (core(signed), core(own)) else {
        return Err(SignatureError::NoVersion);
    };
    if signed_core < own_core {
        return Err(SignatureError::Downgrade {
            signed: signed.to_owned(),
            own: own.to_owned(),
        });
    }
    Ok(())
}

/// `v1.2.3-beta+4` as `(1, 2, 3)`; `None` unless the part before any `-` or `+` is exactly
/// three decimal numbers. A pre-release counts as its release: the same core is no downgrade.
fn core(version: &str) -> Option<(u64, u64, u64)> {
    let version = version.strip_prefix('v').unwrap_or(version);
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u64>().ok())
            .flatten()
    });
    let parsed = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(parsed)
}

/// Tauri's base64 wrapping of a minisign file undone: the text inside, or `None` when the
/// bytes aren't base64 of UTF-8 text.
fn untauri(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    String::from_utf8(decode_base64(&compact(text))?).ok()
}

#[cfg(test)]
mod tests;
