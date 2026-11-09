//! Detection (the brief, "Detection"): whether the lawyer already has a Claude Code or Codex
//! login. It starts no CLI, so it writes nothing into the lawyer's own config and sends nothing
//! over the network: for each agent it is one stat (never an open) of the login file where the
//! CLI keeps it by default (`super::reuse`, the one file that may name them).
//!
//! Why no CLI: Claude Code's `auth status` writes into the lawyer's folders, even offline. On an
//! expired token it takes and removes its refresh locks (`~/.claude/.oauth_refresh.lock`, an
//! `.owner` record written through a temp file and a rename, and `~/.claude.lock`), and online
//! the refresh rewrites `~/.claude/.credentials.json`; with no `CLAUDE_CONFIG_DIR` it also
//! rewrites `~/.claude.json` and writes a backup the first time a version meets the home. Codex's
//! `login status` writes `~/.codex/tmp/arg0` even when signed out. (Decision 0046, "Detection".)
//!
//! - Claude: `~/.claude/.credentials.json` (`%USERPROFILE%\.claude` on Windows) a regular file
//!   is `LoginFound { from: ClaudeCode, email: None }`; anything else (missing, a link, a folder)
//!   is `SignedOut { found: NoLogin }`. macOS keeps the login in the Keychain, so nothing is
//!   found there. With `CLAUDE_CONFIG_DIR` set in the lawyer's environment nothing is found: their
//!   Claude Code keeps its login elsewhere, and "Use it" reads the default store only
//!   (`CLAUDE_SECURESTORAGE_CONFIG_DIR=""`).
//! - Codex: `~/.codex/auth.json` a regular file is `LoginFound { from: Codex, email: None }`, else
//!   `SignedOut { found: NoLogin }`. With `CODEX_HOME` set in the lawyer's environment nothing is
//!   found: "Use it", the split check and the guard address `~/.codex` only.
//!
//! No email, plan or account comes from detection. "Use it" confirms through the agent's own
//! status under the reused environment (the lawyer's explicit choice, so a refresh write there is
//! theirs), which gives the plan and what email it reports; the account itself is enforced by
//! R20's session-start identity check (#554).
//!
//! Detection never starts reuse: it gives `LoginFound`, and only "Use it" reuses.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use crate::cancel::Cancel;
use crate::signin::job::SigninJob;
use crate::signin::{SigninState, reuse};
use crate::status::{Identity, claude_detection, codex_detection};

/// What detection found: the state, and an account it never knows (kept for the callers'
/// shape: always empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Detected {
    /// `LoginFound`, or `SignedOut` with what was seen.
    pub(crate) state: SigninState,
    /// Always empty: detection reads no email or plan.
    pub(crate) identity: Identity,
}

/// Claude's detection, a stat alone: see the module docs. `None` only when cancelled.
pub(crate) fn claude<T>(job: &SigninJob<T>, cancel: &Cancel) -> Option<Detected> {
    detected(
        job,
        cancel,
        "CLAUDE_CONFIG_DIR",
        reuse::claude_login_present,
    )
    .map(|present| found(claude_detection(present)))
}

/// Codex's detection, a stat alone: see the module docs. `None` only when cancelled.
pub(crate) fn codex<T>(job: &SigninJob<T>, cancel: &Cancel) -> Option<Detected> {
    detected(job, cancel, "CODEX_HOME", reuse::lawyer_login_present)
        .map(|present| found(codex_detection(present)))
}

/// Whether the agent's login file is there by `stat` (in the lawyer's home), unless the
/// lawyer's environment moves the agent's folder with `moved_by`; `None` when cancelled.
fn detected<T>(
    job: &SigninJob<T>,
    cancel: &Cancel,
    moved_by: &str,
    stat: fn(&Path) -> bool,
) -> Option<bool> {
    if cancel.is_cancelled() {
        return None;
    }
    let env = &job.config.signin.base_env;
    Some(
        !folder_moved(env, moved_by, cfg!(windows))
            && job
                .config
                .signin
                .lawyer_home()
                .is_some_and(|lawyer| stat(&lawyer)),
    )
}

fn found(state: SigninState) -> Detected {
    Detected {
        state,
        identity: Identity::default(),
    }
}

/// Whether the lawyer's environment `env` sets `name` (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`) to a
/// folder (not empty; names in any case on Windows, as Windows reads them). Their CLI then keeps
/// its login under that folder, while "Use it" takes the default one only; so detection
/// reports nothing found rather than offer a login "Use it" would not take.
pub(crate) fn folder_moved(env: &BTreeMap<OsString, OsString>, name: &str, windows: bool) -> bool {
    env.iter().any(|(key, value)| {
        let key = key.to_string_lossy();
        let named = if windows {
            key.eq_ignore_ascii_case(name)
        } else {
            key == name
        };
        named && !value.is_empty()
    })
}

#[cfg(test)]
mod tests;
