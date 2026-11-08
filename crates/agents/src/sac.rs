//! Codex's bundled ripgrep under Windows Smart App Control (SAC).
//!
//! `@openai/codex`'s Windows package ships the official ripgrep's `rg.exe`, which is unsigned,
//! in `vendor/<triple>/codex-path/`. codex.exe puts that folder first on its own `PATH` as it
//! starts (openai/codex `codex-rs/arg0/src/lib.rs:161-172`), so the model's shell `rg` runs it.
//! With SAC On Windows refuses to start it (error 4551) and may show the
//! lawyer a notification for each try. When the folder is gone, codex prepends nothing: the
//! model's `rg` finds the user's own ripgrep, or none, and Codex's prompt tells it to use
//! alternatives then. Codex's `@` file search walks files in-process and is not affected.
//!
//! So after an install, and at every start, on Windows:
//! - SAC On: `codex-path` is renamed to `codex-path.sac-off` beside it, when its
//!   `rg.exe` is the pinned ripgrep. Another file (another Codex) is left alone, with a warning:
//!   the pin is updated with the Codex pin.
//! - SAC Off or Evaluation: `codex-path.sac-off` is renamed back, so ripgrep returns with no
//!   download. Evaluation blocks nothing, and fresh Windows 11 installs often sit in it: moving
//!   ripgrep there would cut Codex's search down to `findstr` for no protection. If SAC turns
//!   from Evaluation to On during a session, the bundled rg is blocked until the next start.
//! - SAC's state unknown (another value, no value, an unreadable key): nothing moves.
//!
//! Every move is a rename, idempotent and best-effort: a failure is logged and never fails the
//! install or the start. Nothing here is part of an install's checks: the receipt vouches for
//! the folder either way, so `codex-path` must never become a key file. macOS and Linux do
//! nothing.

use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use clauseo_protocol::AgentId;
use sha2::{Digest as _, Sha256};

use crate::config::AgentsConfig;
use crate::integrity::to_hex;
use crate::write;

#[cfg(windows)]
mod windows;

/// The folder codex.exe prepends to its `PATH` (`codex-package.json`'s `pathDir`).
const PATH_DIR: &str = "codex-path";
/// Where that folder is moved: beside it, so off `PATH`.
const MOVED_DIR: &str = "codex-path.sac-off";
/// The ripgrep in it.
const RG: &str = "rg.exe";

/// Codex's Windows layouts in an install folder, each with its `rg.exe`'s SHA-256. Both are the
/// official ripgrep 15.2.0, as `@openai/codex` 0.162.1 ships it (the tarballs
/// `codex-0.162.1-win32-x64.tgz` and `codex-0.162.1-win32-arm64.tgz` from registry.npmjs.org,
/// whose integrity is the lock's), byte for byte the `rg.exe` of BurntSushi/ripgrep's release
/// zips `ripgrep-15.2.0-x86_64-pc-windows-msvc.zip` and
/// `ripgrep-15.2.0-aarch64-pc-windows-msvc.zip`. Update them with every Codex bump.
#[cfg(any(windows, test))]
const LAYOUTS: [(&str, &str); 2] = [
    (
        "node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc",
        "14231169855ec5205cf5a1b6f1db358ff4aed4247c86b69ce8aae647c77f6680",
    ),
    (
        "node_modules/@openai/codex-win32-arm64/vendor/aarch64-pc-windows-msvc",
        "d33a29a9ef03c9f4c03be9e8d88498e6e2d2e566d64cdbdef97f9afc8f13120c",
    ),
];

/// Smart App Control's state: `VerifiedAndReputablePolicyState` under
/// `HKLM\SYSTEM\CurrentControlSet\Control\CI\Policy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SacState {
    /// 0: off.
    Off,
    /// 1: on, enforcing.
    On,
    /// 2: evaluation: Windows watches and blocks nothing yet.
    Evaluation,
    /// Another value (`Some`), or no value or an unreadable key (`None`).
    Unknown(Option<u32>),
}

impl SacState {
    /// The state a read value stands for; `None` is no value or an unreadable key.
    #[cfg(any(windows, test))]
    pub(crate) fn from_value(value: Option<u32>) -> SacState {
        match value {
            Some(0) => SacState::Off,
            Some(1) => SacState::On,
            Some(2) => SacState::Evaluation,
            other => SacState::Unknown(other),
        }
    }
}

/// Tests only: the SAC state and the layouts (folder under the install folder, `rg.exe`'s
/// SHA-256 in hex) used instead of the registry and [`LAYOUTS`], on any OS.
#[cfg(feature = "test-support")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SacForTests {
    /// The state read.
    pub state: SacState,
    /// Each layout's folder, `/`-separated below the install folder, and its pin.
    pub layouts: Vec<(String, String)>,
}

/// What [`apply`] did to one layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// SAC is on: `codex-path` was moved aside.
    Moved,
    /// SAC is off or in evaluation: `codex-path.sac-off` was moved back.
    Restored,
    /// SAC is on but `rg.exe` is not the pinned file (its hash, or why it did not read).
    Mismatch(String),
    /// SAC's state is unknown: nothing moved.
    Unknown(Option<u32>),
    /// A rename failed (what, and the error).
    Failed(String),
}

impl Outcome {
    /// The level its log line takes: a mismatch and a failure warn, the rest inform.
    pub(crate) fn level(&self) -> tracing::Level {
        match self {
            Outcome::Mismatch(_) | Outcome::Failed(_) => tracing::Level::WARN,
            Outcome::Moved | Outcome::Restored | Outcome::Unknown(_) => tracing::Level::INFO,
        }
    }
}

/// Settles `folder` (an install folder, or the temp folder about to become one) for this
/// machine's SAC state: Windows only, and tests through [`SacForTests`]. Never fails.
pub(crate) fn settle(config: &AgentsConfig, folder: &Path, agent: &AgentId) {
    #[cfg(feature = "test-support")]
    if let Some(fake) = &config.sac_for_tests {
        let layouts: Vec<(PathBuf, &str)> = fake
            .layouts
            .iter()
            .map(|(dir, pin)| (rel(folder, dir), pin.as_str()))
            .collect();
        log(agent, &apply(&layouts, || fake.state));
        return;
    }
    #[cfg(not(feature = "test-support"))]
    let _: &AgentsConfig = config;
    #[cfg(windows)]
    {
        let layouts: Vec<(PathBuf, &str)> = LAYOUTS
            .iter()
            .map(|&(dir, pin)| (rel(folder, dir), pin))
            .collect();
        log(agent, &apply(&layouts, windows::read_state));
    }
    #[cfg(not(windows))]
    let _: (&Path, &AgentId) = (folder, agent);
}

/// `dir` (`/`-separated) below `folder`.
fn rel(folder: &Path, dir: &str) -> PathBuf {
    dir.split('/')
        .fold(folder.to_path_buf(), |path, part| path.join(part))
}

/// Applies the rules to each layout (its vendor folder and its `rg.exe`'s pin) that has a
/// `codex-path` or a `codex-path.sac-off`. SAC's state is read only when one does, so an
/// install with no Codex (Claude's) reads nothing and logs nothing.
pub(crate) fn apply(layouts: &[(PathBuf, &str)], read: impl FnOnce() -> SacState) -> Vec<Outcome> {
    let present: Vec<&(PathBuf, &str)> = layouts
        .iter()
        .filter(|(dir, _)| dir.join(PATH_DIR).is_dir() || dir.join(MOVED_DIR).is_dir())
        .collect();
    if present.is_empty() {
        return Vec::new();
    }
    let state = read();
    present
        .into_iter()
        .filter_map(|(dir, pin)| apply_one(dir, pin, state))
        .collect()
}

/// One layout; `None` when there is nothing to do (already settled).
fn apply_one(dir: &Path, pin: &str, state: SacState) -> Option<Outcome> {
    let path_dir = dir.join(PATH_DIR);
    let moved = dir.join(MOVED_DIR);
    match state {
        SacState::On => {
            if !path_dir.is_dir() {
                return None;
            }
            match sha256_hex(&path_dir.join(RG)) {
                Ok(hash) if hash == pin => {}
                Ok(hash) => return Some(Outcome::Mismatch(hash)),
                Err(err) => return Some(Outcome::Mismatch(err.to_string())),
            }
            // A leftover from an interrupted move goes first: the rename needs a free name.
            if let Err(err) = write::remove_dir_all(&moved) {
                return Some(Outcome::Failed(format!("clearing {MOVED_DIR}: {err}")));
            }
            Some(match write::rename(&path_dir, &moved) {
                Ok(()) => Outcome::Moved,
                Err(err) => Outcome::Failed(format!("moving {PATH_DIR} aside: {err}")),
            })
        }
        SacState::Off | SacState::Evaluation => {
            if path_dir.exists() || !moved.is_dir() {
                return None;
            }
            Some(match write::rename(&moved, &path_dir) {
                Ok(()) => Outcome::Restored,
                Err(err) => Outcome::Failed(format!("moving {PATH_DIR} back: {err}")),
            })
        }
        SacState::Unknown(value) => Some(Outcome::Unknown(value)),
    }
}

/// The SHA-256 of the file at `path`, in lowercase hex.
fn sha256_hex(path: &Path) -> io::Result<String> {
    let mut file = write::open_read(path)?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        hasher.update(chunk.get(..read).unwrap_or_default());
    }
    Ok(to_hex(&hasher.finalize()))
}

/// One line per outcome, at its [`Outcome::level`].
fn log(agent: &AgentId, outcomes: &[Outcome]) {
    for outcome in outcomes {
        let line = describe(outcome);
        if outcome.level() == tracing::Level::WARN {
            tracing::warn!(%agent, "{line}");
        } else {
            tracing::info!(%agent, "{line}");
        }
    }
}

/// `outcome`'s log line.
fn describe(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Moved => format!(
            "Smart App Control is on: Codex's unsigned rg.exe moved off PATH \
             ({PATH_DIR} -> {MOVED_DIR})"
        ),
        Outcome::Restored => format!(
            "Smart App Control is off or in evaluation: Codex's rg.exe moved back ({MOVED_DIR} -> {PATH_DIR})"
        ),
        Outcome::Mismatch(found) => format!(
            "Smart App Control is on, but Codex's rg.exe is not the pinned ripgrep ({found}): \
             left in place; update the pin in sac.rs with the Codex pin"
        ),
        Outcome::Unknown(value) => format!(
            "Smart App Control's state is unknown ({value:?}): Codex's rg.exe left as it is"
        ),
        Outcome::Failed(detail) => format!("Codex's rg.exe under Smart App Control: {detail}"),
    }
}

#[cfg(test)]
mod tests;
