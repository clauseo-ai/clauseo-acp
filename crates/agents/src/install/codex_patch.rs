//! codex-acp's sandboxed shell gets the network when the session has web search.
//!
//! codex-acp 2.2.2 hardcodes `networkAccess: false` in the sandbox policy of its
//! `workspace-write` and `agent` modes (`dist/index.js:34287, 34303`) and sends that policy with
//! every turn, so no Codex config can turn the shell's network on. After `npm ci`, before the
//! marker, the install rewrites those two lines to
//! `networkAccess: process.env.CLAUSEO_CODEX_NETWORK === "1",`; the host sets that variable
//! when the session's web search is on (`clauseo-host`'s `agent::codex::launch_env`). The
//! read-only mode's `networkAccess: false` has no comma and stays.
//!
//! The file must be 2.2.2's, byte for byte ([`PRISTINE`], the tarball's file, the same on
//! every platform), and the result must be [`PATCHED`]. Each line is found by its mode's text
//! (`static WorkspaceWrite = new _AgentMode(`, `static Agent = new _AgentMode(`), never by line
//! number. Anything else fails the try as a deterministic failure (`TryError::stop`). The new
//! bytes go to a temp file beside it, renamed over it, so no half-written file is left. A file
//! that already has [`PATCHED`] is left as it is.
//!
//! An install is patched when its lock holds codex-acp ([`patched_by`]). Its install hash then
//! includes [`TAG`] (`Snapshot::install_hash`), so an install made before the patch has another
//! folder and is redone once. A codex-acp bump fails `pinned_codex_acp_is_the_patched_version`
//! until the patch is redone for it, or removed once codex-acp takes the setting itself.

use std::fmt;
use std::path::Path;

use clauseo_protocol::AgentId;
use sha2::{Digest as _, Sha256};

use super::TryError;
use crate::config::AgentsConfig;
use crate::integrity::to_hex;
use crate::pins::AgentPins;
use crate::write;

#[cfg(test)]
mod tests;

/// The package patched.
pub(crate) const PACKAGE: &str = "@agentclientprotocol/codex-acp";
/// Its version the patch is made for.
pub(crate) const VERSION: &str = "2.2.2";
/// The file patched, `/`-separated below the install folder.
const FILE: &str = "node_modules/@agentclientprotocol/codex-acp/dist/index.js";
/// Added to the install hash of an agent that is patched: change it when the patch changes.
pub(crate) const TAG: &str = "codex-shell-network-1";
/// The variable the patched file reads ([`TO`] spells it out).
#[cfg(test)]
const VAR: &str = "CLAUSEO_CODEX_NETWORK";
/// codex-acp 2.2.2's `dist/index.js`, as its tarball (the lock's integrity) holds it.
pub(crate) const PRISTINE: &str =
    "6b1bb4e7e9caaf4aeca1042ae4aec2dc2433b0b0e4b1614b38a4d41699902aaa";
/// The same file once patched.
pub(crate) const PATCHED: &str = "c60271fcbdf7696fc96462d0194311530c8f1d8451474f9d04a5d1a6221ebedd";

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

/// The two hashes a file is checked against, in hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pins<'a> {
    pub(crate) pristine: &'a str,
    pub(crate) patched: &'a str,
}

/// codex-acp 2.2.2's pins.
pub(crate) const PINS: Pins<'static> = Pins {
    pristine: PRISTINE,
    patched: PATCHED,
};

/// Tests only: the file and the pins used instead of [`FILE`] and [`PINS`], for `agent`'s
/// install, on the fixture's packages. Outside `test-support` the field does not exist.
#[cfg(feature = "test-support")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexPatchForTests {
    /// The agent whose install is patched.
    pub agent: AgentId,
    /// The file, `/`-separated below the install folder.
    pub file: String,
    /// The SHA-256 of the file before the patch, in hex.
    pub pristine: String,
    /// The SHA-256 of the file after it, in hex.
    pub patched: String,
}

/// Why a file was not patched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PatchError {
    /// The file is neither the pristine one nor the patched one (its SHA-256).
    Unexpected(String),
    /// A mode's text is not there exactly once, or its line is not in it.
    Anchor(&'static str),
    /// The patched bytes are not the pinned result (their SHA-256).
    Result(String),
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatchError::Unexpected(found) => write!(
                f,
                "codex-acp's dist/index.js is not the {VERSION} file Clauseo patches \
                 (SHA-256 {found})"
            ),
            PatchError::Anchor(mode) => write!(
                f,
                "codex-acp's dist/index.js has no single {mode} mode with its network line"
            ),
            PatchError::Result(found) => write!(
                f,
                "codex-acp's patched dist/index.js is not the expected file (SHA-256 {found})"
            ),
        }
    }
}

/// What [`patch_file`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The two lines were replaced.
    Patched,
    /// The file was already patched: left as it is.
    AlreadyPatched,
}

/// Whether installs of these pins are patched: their lock holds codex-acp.
pub(crate) fn patched_by(pins: &AgentPins) -> bool {
    let path = format!("node_modules/{PACKAGE}");
    pins.packages().iter().any(|package| package.path == path)
}

/// The SHA-256 of `bytes` in hex.
fn sha256(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

/// `bytes` patched: `None` when they already are, an error when they are neither the pristine
/// file nor the patched one, or when the result is not `pins.patched`.
pub(crate) fn patch(bytes: &[u8], pins: Pins<'_>) -> Result<Option<Vec<u8>>, PatchError> {
    let found = sha256(bytes);
    if found == pins.patched {
        return Ok(None);
    }
    if found != pins.pristine {
        return Err(PatchError::Unexpected(found));
    }
    let mut at = Vec::with_capacity(MODES.len());
    for mode in MODES {
        let name = mode_name(mode);
        let start = only(bytes, mode).ok_or(PatchError::Anchor(name))?;
        let body = start + mode.len();
        let end = find(bytes, NEXT, body).unwrap_or(bytes.len());
        let line = find(&bytes[..end], FROM, body).ok_or(PatchError::Anchor(name))?;
        at.push(line);
    }
    at.sort_unstable();
    let mut out = Vec::with_capacity(bytes.len() + MODES.len() * (TO.len() - FROM.len()));
    let mut from = 0;
    for line in at {
        out.extend_from_slice(&bytes[from..line]);
        out.extend_from_slice(TO);
        from = line + FROM.len();
    }
    out.extend_from_slice(&bytes[from..]);
    let result = sha256(&out);
    if result != pins.patched {
        return Err(PatchError::Result(result));
    }
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

/// Where `needle` is in `hay`, when it is there exactly once.
fn only(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let first = find(hay, needle, 0)?;
    find(hay, needle, first + 1).is_none().then_some(first)
}

/// Patches the file at `path` in place: the new bytes to a temp file beside it, renamed over
/// it, with its permissions kept.
pub(crate) fn patch_file(path: &Path, pins: Pins<'_>) -> Result<Outcome, FileError> {
    let bytes = std::fs::read(path).map_err(FileError::Io)?;
    let Some(out) = patch(&bytes, pins).map_err(FileError::Patch)? else {
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
        let pins = Pins {
            pristine: &fake.pristine,
            patched: &fake.patched,
        };
        return run(config, &rel(folder, &fake.file), agent, pins);
    }
    if !patched_by(pins) {
        return Ok(());
    }
    run(config, &rel(folder, FILE), agent, PINS)
}

/// [`patch_file`] on `path`, logged, its error a try's.
fn run(
    config: &AgentsConfig,
    path: &Path,
    agent: &AgentId,
    pins: Pins<'_>,
) -> Result<(), TryError> {
    match patch_file(path, pins) {
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
