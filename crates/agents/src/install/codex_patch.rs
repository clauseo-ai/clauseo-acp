//! codex-acp's sandboxed shell gets the network when the session has web search.
//!
//! codex-acp 2.12.0 (as 2.10.0 and 2.2.2 before it) hardcodes `networkAccess: false` in the
//! sandbox policy of its `workspace-write` and `agent` modes (`dist/index.js:43503, 43519`) and
//! sends that policy with every turn, so no Codex config can turn the shell's network on. After
//! `npm ci`, before the marker, the install rewrites those two lines to
//! `networkAccess: process.env.CLAUSEO_CODEX_NETWORK === "1",`; the host sets that variable
//! when the session's web search is on (`clauseo-host`'s `agent::codex::launch_env`). The
//! read-only mode's `networkAccess: false` (`:43489`) is outside both modes and stays.
//!
//! The patch goes by the file's text, not its hash. Each mode is found by the text that starts
//! it (`static WorkspaceWrite = new _AgentMode(`, `static Agent = new _AgentMode(`), exactly
//! once in the file; inside it, up to the next `static `, there must be one
//! `type: "workspaceWrite"` and one network line, either still `networkAccess: false,` or
//! already patched. Anything else fails the try as a deterministic failure (`TryError::stop`).
//! A file whose two modes are already patched is left as it is. The new bytes go to a temp file
//! beside it, renamed over it, so no half-written file is left.
//!
//! An install is patched when its lock holds codex-acp ([`patched_by`]). Its install hash then
//! includes [`TAG`] (`Snapshot::install_hash`), so an install made before the patch has another
//! folder and is redone once. A codex-acp bump fails `pinned_codex_acp_is_the_patched_version`
//! until the patch is checked against it, or removed once codex-acp takes the setting itself.
//!
//! [`check_codex_patch`] runs the same pattern on an install's file and `node --check` on the
//! patched copy, for the engine watch's check of a new codex-acp version; it changes nothing.

use std::fmt;
use std::path::Path;

use clauseo_protocol::AgentId;

use super::TryError;
use crate::config::AgentsConfig;
use crate::pins::AgentPins;
use crate::write;

mod check;
#[cfg(test)]
mod tests;

pub use check::{CheckError, check_codex_patch};

/// The package patched.
pub(crate) const PACKAGE: &str = "@agentclientprotocol/codex-acp";
/// Its version the patch was last checked against.
pub(crate) const VERSION: &str = "2.12.0";
/// The file patched, `/`-separated below the install folder.
const FILE: &str = "node_modules/@agentclientprotocol/codex-acp/dist/index.js";
/// Added to the install hash of an agent that is patched: change it when the patch changes.
pub(crate) const TAG: &str = "codex-shell-network-1";
/// The variable the patched file reads ([`TO`] spells it out).
#[cfg(test)]
const VAR: &str = "CLAUSEO_CODEX_NETWORK";
/// The line replaced, in each mode.
const FROM: &[u8] = b"networkAccess: false,";
/// What replaces it.
const TO: &[u8] = b"networkAccess: process.env.CLAUSEO_CODEX_NETWORK === \"1\",";
/// The modes whose line is replaced, by the text that starts each one.
const MODES: [&[u8]; 2] = [
    b"static WorkspaceWrite = new _AgentMode(",
    b"static Agent = new _AgentMode(",
];
/// What starts the next mode, so a mode's line is looked for only inside its own.
const NEXT: &[u8] = b"static ";
/// The sandbox policy each patched mode must have.
const POLICY: &[u8] = b"type: \"workspaceWrite\"";

/// Tests only: the file used instead of [`FILE`], for `agent`'s install, on the fixture's
/// packages. Outside `test-support` the field does not exist.
#[cfg(feature = "test-support")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexPatchForTests {
    /// The agent whose install is patched.
    pub agent: AgentId,
    /// The file, `/`-separated below the install folder.
    pub file: String,
}

/// Why a file was not patched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PatchError {
    /// A mode's text is not there exactly once, or its policy or network line is not in it
    /// exactly once.
    Anchor(&'static str),
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatchError::Anchor(mode) => write!(
                f,
                "codex-acp's dist/index.js (Clauseo patches {VERSION}) has no single {mode} \
                 mode with one network line"
            ),
        }
    }
}

/// What [`patch_file`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The lines not yet patched were replaced.
    Patched,
    /// The file was already patched: left as it is.
    AlreadyPatched,
}

/// Whether installs of these pins are patched: their lock holds codex-acp.
pub(crate) fn patched_by(pins: &AgentPins) -> bool {
    let path = format!("node_modules/{PACKAGE}");
    pins.packages().iter().any(|package| package.path == path)
}

/// `bytes` patched: `None` when both modes already are, an error when a mode, its policy or
/// its network line is not there exactly once.
pub(crate) fn patch(bytes: &[u8]) -> Result<Option<Vec<u8>>, PatchError> {
    let mut at = Vec::with_capacity(MODES.len());
    for mode in MODES {
        let name = mode_name(mode);
        let start = only(bytes, mode).ok_or(PatchError::Anchor(name))?;
        let from = start + mode.len();
        let end = find(bytes, NEXT, from).unwrap_or(bytes.len());
        let body = &bytes[from..end];
        if count(body, POLICY) != 1 {
            return Err(PatchError::Anchor(name));
        }
        match (count(body, FROM), count(body, TO)) {
            (1, 0) => at.extend(find(body, FROM, 0).map(|line| from + line)),
            (0, 1) => {}
            _ => return Err(PatchError::Anchor(name)),
        }
    }
    if at.is_empty() {
        return Ok(None);
    }
    at.sort_unstable();
    let mut out = Vec::with_capacity(bytes.len() + at.len() * (TO.len() - FROM.len()));
    let mut from = 0;
    for line in at {
        out.extend_from_slice(&bytes[from..line]);
        out.extend_from_slice(TO);
        from = line + FROM.len();
    }
    out.extend_from_slice(&bytes[from..]);
    Ok(Some(out))
}

/// The mode's id as codex-acp names it, for an error.
fn mode_name(mode: &[u8]) -> &'static str {
    if mode == MODES[0] {
        "workspace-write"
    } else {
        "agent"
    }
}

/// Where `needle` is in `hay`, at or after `from`.
fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| at + from)
}

/// How many times `needle` is in `hay`, not overlapping.
fn count(hay: &[u8], needle: &[u8]) -> usize {
    let mut n = 0;
    let mut from = 0;
    while let Some(at) = find(hay, needle, from) {
        n += 1;
        from = at + needle.len();
    }
    n
}

/// Where `needle` is in `hay`, when it is there exactly once.
fn only(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let first = find(hay, needle, 0)?;
    find(hay, needle, first + 1).is_none().then_some(first)
}

/// Patches the file at `path` in place: the new bytes to a temp file beside it, renamed over
/// it, with its permissions kept.
pub(crate) fn patch_file(path: &Path) -> Result<Outcome, FileError> {
    let bytes = std::fs::read(path).map_err(FileError::Io)?;
    let Some(out) = patch(&bytes).map_err(FileError::Patch)? else {
        return Ok(Outcome::AlreadyPatched);
    };
    write::replace_atomic(path, &out).map_err(FileError::Io)?;
    Ok(Outcome::Patched)
}

/// Why [`patch_file`] failed.
#[derive(Debug)]
pub(crate) enum FileError {
    /// Reading, writing or renaming.
    Io(std::io::Error),
    /// The bytes.
    Patch(PatchError),
}

/// Patches `folder` (the temp folder about to become `agent`'s install) when `pins` are
/// patched ([`patched_by`]); any other install is left alone. A missing or unexpected file
/// stops the try.
pub(crate) fn apply(
    config: &AgentsConfig,
    folder: &Path,
    agent: &AgentId,
    pins: &AgentPins,
) -> Result<(), TryError> {
    #[cfg(feature = "test-support")]
    if let Some(fake) = &config.codex_patch_for_tests {
        if fake.agent != *agent {
            return Ok(());
        }
        return run(config, &rel(folder, &fake.file), agent);
    }
    if !patched_by(pins) {
        return Ok(());
    }
    run(config, &rel(folder, FILE), agent)
}

/// [`patch_file`] on `path`, logged, its error a try's.
fn run(config: &AgentsConfig, path: &Path, agent: &AgentId) -> Result<(), TryError> {
    match patch_file(path) {
        Ok(outcome) => {
            let what = match outcome {
                Outcome::Patched => "patched",
                Outcome::AlreadyPatched => "already patched",
            };
            tracing::info!(%agent, outcome = what, "agent install: codex-acp's shell network");
            Ok(())
        }
        Err(FileError::Patch(err)) => Err(TryError::stop(&err.to_string(), config)),
        Err(FileError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => Err(
            TryError::stop("codex-acp's dist/index.js is not in the install", config),
        ),
        Err(FileError::Io(err)) => Err(TryError::io("patching codex-acp", &err, config)),
    }
}

/// `file` (`/`-separated) below `folder`.
fn rel(folder: &Path, file: &str) -> std::path::PathBuf {
    file.split('/')
        .fold(folder.to_path_buf(), |path, part| path.join(part))
}
