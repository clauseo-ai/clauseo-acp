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
    /// The last set-up was blocked by a setting only whoever looks after the computer can
    /// change (D-B4), and nothing has been set up since: `SetUpBlocked`. Trying again asks for
    /// an administrator again, and fails again until the setting changes.
    Blocked(Blocked),
}

/// Why a set-up step failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    /// The lawyer declined the prompt (`ERROR_CANCELLED`, 1223). A logon type not granted (1385)
    /// is [`SetupError::Blocked`], naming the right (D-B4).
    #[error("sandbox set-up was refused")]
    Refused,
    /// Anything else; what failed.
    #[error("sandbox set-up failed: {0}")]
    Failed(String),
    /// A Windows setting blocks the set-up, which only whoever looks after the computer can
    /// change (D-B4): a policy blocking the set-up program (1260), or a logon right the sandbox
    /// users lack (1385).
    #[error("sandbox set-up is blocked: {0}")]
    Blocked(Blocked),
}

/// What blocks the set-up, named so firm IT can act on it (D-B4). Its words name the setting
/// to change, never a path or a SID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Blocked {
    /// `ERROR_LOGON_TYPE_NOT_GRANTED` (1385) when set-up logs a sandbox user on: Windows'
    /// "Allow log on locally" right doesn't include them (the interactive logon set-up uses).
    LogonRight,
    /// `ERROR_ACCESS_DISABLED_BY_POLICY` (1260) when the app starts the set-up program: a
    /// Windows policy (`AppLocker`, Software Restriction Policies) blocks it.
    Policy,
}

impl std::fmt::Display for Blocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Blocked::LogonRight => {
                "Windows' \"Allow log on locally\" right doesn't include the group \
                 ClauseoSandboxUsers; whoever looks after this computer can add it"
            }
            Blocked::Policy => {
                "a Windows policy blocks clauseo-sandbox-setup.exe, Clauseo's set-up program; \
                 whoever looks after this computer can allow it"
            }
        })
    }
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
