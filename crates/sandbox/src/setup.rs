//! The sandbox's one-time set-up, as every OS sees it: the [`SandboxSetup`] seam R17 holds, what
//! a set-up is in ([`SetupState`]), and why a set-up step failed ([`SetupError`]).
//!
//! Windows' tier B is the one set-up today (S15): an elevated step makes the `ClauseoSandbox`
//! user once per machine. `windows::UserSetup` (S15 PR4) implements the trait; until then
//! `windows::setup_state` reads the same markers and record its `state` will. macOS and Linux
//! need no set-up and have no implementation.

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
    /// Set up for this user, at the current version, and the machine's record names the same
    /// sandbox user: tier B.
    Ready,
    /// Never set up for this user, or the sandbox user is gone: `NeedsSetUp`.
    Needed,
    /// The last set-up's prompt was declined: `SetUpRefused`.
    Refused,
    /// A marker or record can't be read or makes no sense, or the last set-up failed; what
    /// happened. `SetUpFailed`.
    Failed(String),
    /// Set up by an older Clauseo, whose set-up must run again: `NeedsSetUp`.
    Outdated {
        /// The version `setup.json` holds.
        found: u32,
        /// The version this build sets up.
        current: u32,
    },
    /// Another Windows user owns the machine's sandbox user (D5: the first user owns it), so this
    /// user stays on tier C: `SetUpFailed`.
    SetUpByOtherUser,
}

/// Why a set-up step failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    /// The lawyer declined the prompt, or Windows refused to raise it (error 1385).
    #[error("sandbox set-up was refused")]
    Refused,
    /// Anything else; what failed.
    #[error("sandbox set-up failed: {0}")]
    Failed(String),
}

/// One protected name moved aside from the matter (S15 PR3's set-aside: `windows::after_session`
/// and `windows::before_launch`; R17 shows one notice per launch that moved any): a protected
/// name at the matter's root, or a `.git`, `.claude` or `.mcp.json` at any depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetAside {
    /// Where the name was, in the matter.
    pub from: PathBuf,
    /// Where it went: `<data_dir>\sandbox\set-aside\<matter>\<timestamp>\...`.
    pub to: PathBuf,
}
