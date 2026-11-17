//! The token files a launch writes in its run folder (`<data>/run/<session id>/`), in one place:
//! the case-law relay's (R24, `actor/turn/launch_checks/relay.rs`) and the version-history
//! listener's (`actor/turn/launch_checks/versions.rs`). `clauseo-mcp connector` reads each
//! itself, so the agent's own tools never need one, and a token read through them would land in
//! the transcript.
//!
//! Every deny list names each of them: Claude's data-folder rules (`agent::data_fence::DENIED`,
//! from which the Read and Edit rules and the Windows shell rules are derived), Codex's fence
//! (`agent::codex_fence`: the own run folder's by path, every session's by `token_globs`) and
//! the test folders that stand in for a data folder. `tests.rs` fails when any of those lists names a
//! token file this list does not hold, or leaves out one it does.

/// The case-law relay's token file.
pub(crate) const RELAY_TOKEN: &str = "relay-token";
/// The version-history listener's token file.
pub(crate) const VERSIONS_TOKEN: &str = "versions-token";

/// Every token file a run folder may hold.
pub(crate) const RUN_TOKEN_FILES: [&str; 2] = [RELAY_TOKEN, VERSIONS_TOKEN];

#[cfg(test)]
mod tests;
