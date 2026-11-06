//! The sandbox's one-time set-up, as every OS sees it: the [`SandboxSetup`] seam R17 holds, what
//! a set-up is in ([`SetupState`]), and why a set-up step failed ([`SetupError`]).
//!
//! Windows' tier B is the one set-up today (S15, design B): an elevated step makes a pool of local
//! users, `ClauseoSb01` and on (4 at first, 4 more at a time, at most 32), in the group
//! `ClauseoSandboxUsers`, and records them in `HKLM\SOFTWARE\Clauseo\Sandbox` with the user who
//! owns them (`windows::pool`). Each matter runs as its own pool user, at Low integrity.
//! `windows::UserSetup` (S15 `PR4h`) implements the trait on Windows: its `state` reads the
//! marker and record (`windows::setup_state`), and `run` starts the elevated setup exe. macOS
//! and Linux need no set-up and have no implementation.

use std::path::PathBuf;

use clauseo_protocol::MatterId;

/// The set-up seam (agreed with R17): the host holds an `Option<Arc<dyn SandboxSetup>>` and asks
/// [`detect`](crate::detect) for the tier through it.
pub trait SandboxSetup: Send + Sync {
    /// What the set-up is in now, read from its markers and records on each call and never
    /// cached, so a set-up that finishes shows at the next tier query. Never prompts or writes.
    fn state(&self) -> SetupState;
    /// Runs the set-up: raises one UAC prompt and blocks until it ends. R17 calls it off the
    /// actor thread.
    fn run(&self) -> Result<(), SetupError>;
    /// Forgets one matter: its SID and its permission entries. Needs no admin.
    fn forget_matter(&self, matter: &MatterId) -> Result<(), SetupError>;
    /// Undoes the set-up (S15a): one UAC prompt.
    fn remove(&self) -> Result<(), SetupError>;
}

/// What the set-up is in. Every state but [`SetupState::Ready`] keeps the machine on tier C, with
/// the [`NoSandbox`](crate::NoSandbox) reason [`detect`](crate::detect) maps it to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupState {
    /// Set up for this user, at the current version, and the machine's record names this user as
    /// owner and every pool user the marker does: tier B.
    Ready,
    /// Never set up for this user, or the record no longer names the pool: `NeedsSetUp`.
    Needed,
    /// The last set-up's prompt was declined: `SetUpRefused`.
    Refused,
    /// A marker or record can't be read or makes no sense, a removal stopped part way, or the
    /// last set-up failed; what happened. `SetUpFailed`.
    Failed(String),
    /// Set up by an older Clauseo, whose set-up must run again: `NeedsSetUp`.
    Outdated {
        /// The version `setup.json` holds.
        found: u32,
        /// The version this build sets up.
        current: u32,
    },
    /// Another Windows user owns the machine's pool (D5: the first user owns it), so this user
    /// stays on tier C: `SetUpFailed`.
    SetUpByOtherUser,
}

/// Why a set-up step failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    /// The lawyer declined the prompt (`ERROR_CANCELLED`, 1223). A logon type not granted (1385)
    /// is a [`SetupError::Failed`] naming the right (D-B4).
    #[error("sandbox set-up was refused")]
    Refused,
    /// Anything else; what failed.
    #[error("sandbox set-up failed: {0}")]
    Failed(String),
}

/// One protected name moved aside from the matter (S15 PR3's set-aside: `windows::after_session`
/// and `windows::before_launch`; R17 shows one notice per launch that moved any): a protected
/// name at the matter's root, or a `.git`, `.claude`, `.mcp.json`, `CLAUDE.md` or `AGENTS.md`
/// (in any letter case) at any depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetAside {
    /// Where the name was, in the matter.
    pub from: PathBuf,
    /// Where it went: `<data_dir>\sandbox\set-aside\<matter>\<timestamp>\...`.
    pub to: PathBuf,
}
