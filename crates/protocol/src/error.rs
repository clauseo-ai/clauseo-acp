//! The crate's one error type: parse and validation failures of the values this crate defines.
//! Host failures are not here; they are [`crate::HostError`] with an [`crate::ErrorCode`]. An
//! `Error` maps to no code by itself: the caller picks one, [`crate::HostError::invalid_command`]
//! for a command's own input, `store.corrupt` for a stored value that does not read back.

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
    /// An envelope's `schema_version` is older than this version and no upcaster reads it.
    #[error("Envelope: no upcaster reads schema_version {version}")]
    NoUpcaster {
        /// The version found.
        version: u32,
    },
    /// A value cannot be written as compact JSON text: a map key in it is a list or a map, or it
    /// nests deeper than [`MAX_RAW_ENCODE_DEPTH`](crate::MAX_RAW_ENCODE_DEPTH). Holds the JSON
    /// encoder's message.
    #[error("no JSON text form: {0}")]
    NotJson(String),
}
