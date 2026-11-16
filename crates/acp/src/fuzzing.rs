//! Internals the fuzz targets in `fuzz/` call directly, built only with the `fuzzing` feature,
//! which only the fuzz crate turns on, and hidden from the docs. Not part of the crate's
//! interface: it changes with the internals.

/// What the start of a `session/update` line over the line cap says, as the line reader reads it
/// (`line/oversize.rs`), for the fuzz target `acp_skipped_update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedUpdate {
    /// `params.sessionId`, when the start showed it.
    pub session: Option<String>,
    /// `params.update.sessionUpdate` is `tool_call_update` (`true`) or another kind.
    pub tool_call_update: Option<bool>,
    /// `params.update.toolCallId`, when the start showed it.
    pub tool_call: Option<String>,
    /// `params.update.status`, when it is one of ACP's four.
    pub status: Option<&'static str>,
    /// The marker line the reader puts in the skipped line's place, for a line of 9 MiB.
    pub marker: String,
}

/// The longest `sessionId` or `toolCallId` the reader copies from a skipped line, in bytes.
pub const MAX_ID_BYTES: usize = crate::line::MAX_ID_BYTES;

/// The line reader's scanner over `head`, the start of a line over the cap: `None` when the line
/// is not a skippable `session/update` notification (the reader closes the connection then).
pub fn skipped_update(head: &[u8]) -> Option<SkippedUpdate> {
    let read = crate::line::skipped_update(head)?;
    Some(SkippedUpdate {
        marker: read.marker(9 * 1024 * 1024),
        session: read.session,
        tool_call_update: read.tool_call_update,
        tool_call: read.tool_call,
        status: read.status,
    })
}
