//! The crate's one error type: parse and validation failures of the values this crate defines.
//! Host failures are not here; they are [`crate::HostError`] with an [`crate::ErrorCode`].

use crate::ErrorCode;

/// A value failed to parse or broke a rule its type states.
///
/// Every variant names the type it was building (`kind`, the Rust type name such as `"ThreadId"`),
/// so a message read in a log says which field was wrong without the input.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The text is not a UUID in its 36-character hyphenated form.
    #[error("{kind}: not a hyphenated UUID")]
    UuidText {
        /// The type being built.
        kind: &'static str,
    },
    /// A Clauseo id that must be a v7 UUID has another version or variant.
    #[error("{kind}: expected a v7 UUID, got version {version}")]
    NotV7 {
        /// The type being built.
        kind: &'static str,
        /// The version nibble found.
        version: usize,
    },
    /// An id that may be any UUID was the nil UUID.
    #[error("{kind}: the nil UUID is not an id")]
    NilUuid {
        /// The type being built.
        kind: &'static str,
    },
    /// A number is outside the range its type allows.
    #[error("{kind}: {value} is outside {min}..={max}")]
    OutOfRange {
        /// The type being built.
        kind: &'static str,
        /// The number given.
        value: i128,
        /// The smallest allowed value.
        min: i128,
        /// The largest allowed value.
        max: i128,
    },
    /// A string breaks the format its type states.
    #[error("{kind}: {rule}")]
    Format {
        /// The type being built.
        kind: &'static str,
        /// The rule it broke, as a fixed sentence.
        rule: &'static str,
    },
    /// The text before the `:` of a `StreamId` or `EntryId` names no kind this version knows.
    #[error("{kind}: unknown prefix {prefix:?}")]
    UnknownPrefix {
        /// The type being built.
        kind: &'static str,
        /// The prefix found, before the `:`.
        prefix: String,
    },
    /// The string is not one of the dotted error codes this version knows.
    #[error("unknown error code {code:?}")]
    UnknownErrorCode {
        /// The string given.
        code: String,
    },
    /// A command or query is over one of the size limits `validate()` checks.
    #[error("{kind}: {what} is {size}, over the limit of {max}")]
    TooLarge {
        /// The command, query or type being checked, such as `"SendPrompt"`.
        kind: &'static str,
        /// What was measured, as a fixed phrase such as `"prompt text in bytes"`.
        what: &'static str,
        /// The size found.
        size: usize,
        /// The largest size allowed.
        max: usize,
    },
    /// A value has no JSON text form (a map key that is binary, a list or a map), so it cannot be
    /// written as compact JSON text.
    #[error("no JSON text form")]
    NotJson(#[source] serde_json::Error),
}

impl Error {
    /// The error code a host call returns when this error reaches the boundary.
    ///
    /// Every parse and validation failure is `command.invalid`: the values this crate parses
    /// arrive in commands, and a client that sends a bad one sent an invalid command. The one
    /// exception is [`Error::NotJson`], which only the host meets when it writes a stored
    /// payload as text, so it is `internal`.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::UuidText { .. }
            | Self::NotV7 { .. }
            | Self::NilUuid { .. }
            | Self::OutOfRange { .. }
            | Self::Format { .. }
            | Self::UnknownPrefix { .. }
            | Self::UnknownErrorCode { .. }
            | Self::TooLarge { .. } => ErrorCode::CommandInvalid,
            Self::NotJson(_) => ErrorCode::Internal,
        }
    }
}
