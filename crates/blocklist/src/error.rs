//! Why a list was refused. None of these quotes the list's content beyond a line number.

use std::fmt;

/// Why a list was refused. The caller keeps the list it has.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The `.minisig` file is not a minisign signature, or is larger than
    /// [`MAX_SIGNATURE`](crate::MAX_SIGNATURE).
    SignatureFile,
    /// No key in the set is a key line.
    NoKey,
    /// No key in the set verifies the signature, or the signature is the legacy kind (not
    /// prehashed), or the trusted comment was changed.
    Signature,
    /// The trusted comment is not `clauseo-blocklist v1 generated=… entries=…`.
    Comment,
    /// The `.gz` file does not decompress.
    Gzip,
    /// More than [`MAX_ENTRIES`](crate::MAX_ENTRIES) entries, or more than
    /// [`MAX_TEXT`](crate::MAX_TEXT) bytes decompressed.
    TooBig,
    /// A line that is neither a comment nor `<host> <tag>` with a valid host and tag.
    Malformed {
        /// The line, from 1.
        line: usize,
    },
    /// A host that sorts before the one above it.
    Unsorted {
        /// The line, from 1.
        line: usize,
    },
    /// A host equal to the one above it.
    Duplicate {
        /// The line, from 1.
        line: usize,
    },
    /// The trusted comment's `entries` differs from the list's entry lines.
    EntriesMismatch {
        /// What the trusted comment says.
        said: u32,
        /// How many entry lines the list has (counting stops one past `said`).
        found: u32,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::SignatureFile => f.write_str("the signature file is not a minisign signature"),
            Error::NoKey => f.write_str("no usable public key is compiled in"),
            Error::Signature => f.write_str("the signature does not verify"),
            Error::Comment => f.write_str("the trusted comment is not the v1 form"),
            Error::Gzip => f.write_str("the list does not decompress"),
            Error::TooBig => f.write_str("the list is over the size bound"),
            Error::Malformed { line } => write!(f, "line {line} is malformed"),
            Error::Unsorted { line } => write!(f, "line {line} is out of order"),
            Error::Duplicate { line } => write!(f, "line {line} repeats the host above it"),
            Error::EntriesMismatch { said, found } => {
                write!(f, "the comment says {said} entries, the list has {found}")
            }
        }
    }
}

impl std::error::Error for Error {}
