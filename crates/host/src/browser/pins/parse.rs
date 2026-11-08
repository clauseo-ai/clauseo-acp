//! A manifest's check, in order: its size, the signature (the block list's code, the pins'
//! keys), the trusted comment's form, `schema`, the body, and the comment against the body.
//! Pure: no file, network or clock.

use std::collections::BTreeMap;
use std::fmt;

use clauseo_blocklist::Generated;
use serde::Deserialize;

use super::{MAX_MANIFEST, MAX_ZIP_BYTES, Pin, Pins};
use crate::browser::install::manifest::check_version;

/// The trusted comment's fixed start.
const COMMENT_PREFIX: &str = "clauseo-browser-pins v1 published=";

/// The longest `run` taken: a run number is a few digits.
const MAX_RUN: usize = 20;

/// The schema this host reads.
const SCHEMA: u64 = 1;

/// Why a manifest was refused. None of these quotes the manifest's content.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PinsError {
    /// Over [`MAX_MANIFEST`] bytes.
    TooBig,
    /// The signature file is not one, no key in the set is a key line, or no key verifies.
    Signature(clauseo_blocklist::Error),
    /// The trusted comment is not `clauseo-browser-pins v1 published=… version=… run=<digits>`.
    Comment,
    /// `schema` is not 1.
    Schema,
    /// The body is not JSON of the v1 shape.
    Body,
    /// `published` is not RFC 3339 UTC to the second.
    Published,
    /// `stable.version` is not dotted digits.
    Version,
    /// The trusted comment's `published` or `version` differs from the body's.
    Mismatch,
    /// A pin's `sha256` is not 64 lowercase hex digits, or its `size` is 0 or over the cap.
    Pin,
}

impl fmt::Display for PinsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooBig => f.write_str("the manifest is over the size bound"),
            Self::Signature(error) => write!(f, "the signature: {error}"),
            Self::Comment => f.write_str("the trusted comment is not the v1 form"),
            Self::Schema => f.write_str("the schema is not 1"),
            Self::Body => f.write_str("the body is not the v1 JSON"),
            Self::Published => f.write_str("published is not an RFC 3339 UTC time"),
            Self::Version => f.write_str("the version is not dotted digits"),
            Self::Mismatch => f.write_str("the trusted comment does not match the body"),
            Self::Pin => f.write_str("a pin's sha256 or size is refused"),
        }
    }
}

impl std::error::Error for PinsError {}

/// The schema alone, read first so a later schema's other changes cannot hide it.
#[derive(Deserialize)]
struct Schema {
    schema: u64,
}

/// The v1 body; unknown fields are ignored.
#[derive(Deserialize)]
struct Body {
    published: String,
    stable: Stable,
}

#[derive(Deserialize)]
struct Stable {
    version: String,
    downloads: BTreeMap<String, BTreeMap<String, RawPin>>,
}

#[derive(Deserialize)]
struct RawPin {
    sha256: String,
    size: u64,
}

/// The trusted comment's three values.
struct Comment<'a> {
    published: &'a str,
    version: &'a str,
    run: &'a str,
}

/// [`Pins::verify`]'s work.
pub(super) fn verify(body: &[u8], minisig: &[u8], keys: &[&str]) -> Result<Pins, PinsError> {
    if u64::try_from(body.len()).unwrap_or(u64::MAX) > MAX_MANIFEST {
        return Err(PinsError::TooBig);
    }
    let text =
        clauseo_blocklist::trusted_comment(body, minisig, keys).map_err(PinsError::Signature)?;
    let comment = comment(&text).ok_or(PinsError::Comment)?;
    let schema: Schema = serde_json::from_slice(body).map_err(|_bad| PinsError::Body)?;
    if schema.schema != SCHEMA {
        return Err(PinsError::Schema);
    }
    let parsed: Body = serde_json::from_slice(body).map_err(|_bad| PinsError::Body)?;
    let published = Generated::parse(&parsed.published).ok_or(PinsError::Published)?;
    let version = parsed.stable.version;
    check_version(&version).map_err(|_bad| PinsError::Version)?;
    let numbers = crate::browser::chrome::version(&version).ok_or(PinsError::Version)?;
    if comment.published != parsed.published || comment.version != version {
        return Err(PinsError::Mismatch);
    }
    let downloads = parsed
        .stable
        .downloads
        .into_iter()
        .map(|(name, platforms)| {
            let platforms = platforms
                .into_iter()
                .map(|(platform, raw)| Ok((platform, pin(raw)?)))
                .collect::<Result<_, PinsError>>()?;
            Ok((name, platforms))
        })
        .collect::<Result<_, PinsError>>()?;
    Ok(Pins {
        published,
        version,
        numbers,
        run: comment.run.to_owned(),
        downloads,
    })
}

/// `clauseo-browser-pins v1 published=<p> version=<v> run=<digits>`, exactly: single spaces,
/// `run` 1 to [`MAX_RUN`] ASCII digits. `published` and `version` are checked against the body.
fn comment(text: &str) -> Option<Comment<'_>> {
    let rest = text.strip_prefix(COMMENT_PREFIX)?;
    let (published, rest) = rest.split_once(" version=")?;
    let (version, run) = rest.split_once(" run=")?;
    let digits = (1..=MAX_RUN).contains(&run.len()) && run.bytes().all(|b| b.is_ascii_digit());
    digits.then_some(Comment {
        published,
        version,
        run,
    })
}

/// A pin, its `sha256` 64 lowercase hex digits and its `size` 1 to [`MAX_ZIP_BYTES`].
fn pin(raw: RawPin) -> Result<Pin, PinsError> {
    let hex = raw.sha256.len() == 64
        && raw
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !hex || raw.size == 0 || raw.size > MAX_ZIP_BYTES {
        return Err(PinsError::Pin);
    }
    Ok(Pin {
        sha256: raw.sha256,
        size: raw.size,
    })
}
