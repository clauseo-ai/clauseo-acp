//! Detection (the brief, "Detection"): whether the lawyer already has a Claude Code or Codex
//! login. It never writes into the lawyer's own Claude or Codex config.
//!
//! - Claude: `--cli auth status --json`, with `RunKind::Detect`'s environment: the run's own,
//!   empty config folder (`super::scratch`, `CLAUDE_CONFIG_DIR`) and
//!   `CLAUDE_SECURESTORAGE_CONFIG_DIR=""`, so the login is read from the lawyer's default store
//!   and everything the CLI writes lands in that folder, which goes after the run. Clauseo never
//!   opens the lawyer's global config or settings (they can hold a key), so the CLI does not see
//!   them either: `LoginFound` has no email (the email lives in the global config; "Use it"'s
//!   status under the reused environment gives what it can), and a login set up in the settings
//!   is not seen. `HOME` is the lawyer's; cwd is an empty Clauseo folder, so no project
//!   settings apply: `<data_dir>/agents/signin/detect`. "Exit 0 with `authMethod` `claude.ai` is
//!   reusable. Any other signed-in `authMethod` … is a login set up in the lawyer's
//!   `~/.claude/settings.json`: not reusable, `found: ClaudeSettings`"
//!   (`status::claude_detection`).
//! - Codex: no CLI runs (its `login status` writes `~/.codex/tmp/arg0` even when signed out,
//!   and `app-server` more). A stat (never an open) of the lawyer's login file
//!   (`reuse::lawyer_login_present`, a regular file only, never a link) decides:
//!   `status::codex_detection`. When the lawyer's environment sets `CODEX_HOME`, their Codex keeps
//!   its login there, not in `~/.codex`, where "Use it", the split check and the guard look; so
//!   nothing is found ([`codex_home_moved`]).
//!
//! Detection never starts reuse: it gives `LoginFound`, and only "Use it" reuses.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_process::Timer;

use crate::cancel::Cancel;
use crate::signin::SigninState;
use crate::signin::env::RunKind;
use crate::signin::job::SigninJob;
use crate::signin::scratch::Scratch;
use crate::signin::{claude, reuse};
use crate::status::{Identity, claude_detection, codex_detection};

/// What detection found: the state, and for a reusable login the account and plan the CLI's own
/// status gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Detected {
    /// `LoginFound`, or `SignedOut` with what was seen.
    pub(crate) state: SigninState,
    /// The plan behind a reusable Claude login (empty otherwise, and always for Codex); never an
    /// email.
    pub(crate) identity: Identity,
}

/// The empty Clauseo folder detection runs in, made if missing.
fn folder<T>(job: &SigninJob<T>) -> Option<PathBuf> {
    let folder = job
        .config
        .data_dir
        .join("agents")
        .join("signin")
        .join("detect");
    crate::write::create_private_dirs_under(&job.config.data_dir, &folder).ok()?;
    Some(folder)
}

/// Claude's detection; `None` when it could not tell (no start, killed past 5 s, cancelled, or
/// no config folder of its own could be made: it never runs on the lawyer's).
pub(crate) async fn claude<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
) -> Option<Detected> {
    let folder = folder(job)?;
    // Dropped at the end of this function, or with its future on a timeout or cancel: the
    // folder goes then.
    let scratch = Scratch::create(&job.config.data_dir, &job.ticket.agent, job.now()).ok()?;
    let kind = RunKind::Detect {
        config: scratch.path(),
    };
    let (code, parsed) = claude::status_run(job, cancel, kind, &folder).await?;
    drop(scratch);
    let parsed = parsed.map(|mut parsed| {
        // Never an email: the CLI keeps it in the lawyer's global config, which this run does
        // not see; whatever a CLI printed here would come from somewhere else.
        parsed.email = None;
        parsed
    });
    let state = claude_detection(code, parsed.as_ref());
    let identity = match (&state, parsed) {
        (SigninState::LoginFound { .. }, Some(parsed)) => Identity {
            email: None,
            plan: parsed.subscription_type,
        },
        _ => Identity::default(),
    };
    Some(Detected { state, identity })
}

/// Codex's detection, a stat alone (no run, so nothing to await): see the module docs. `None`
/// only when cancelled.
pub(crate) fn codex<T>(job: &SigninJob<T>, cancel: &Cancel) -> Option<Detected> {
    if cancel.is_cancelled() {
        return None;
    }
    let present = !codex_home_moved(&job.config.signin.base_env, cfg!(windows))
        && job
            .config
            .signin
            .lawyer_home()
            .is_some_and(|lawyer| reuse::lawyer_login_present(&lawyer));
    Some(Detected {
        state: codex_detection(present),
        identity: Identity::default(),
    })
}

/// Whether the lawyer's environment `env` sets `CODEX_HOME` (not empty; names in any case on
/// Windows, as Windows reads them). Their Codex then keeps its login under that folder, while
/// "Use it" links, and the split check and guard compare, `~/.codex`'s file only; so detection
/// reports nothing found rather than offer a login "Use it" would not take.
pub(crate) fn codex_home_moved(env: &BTreeMap<OsString, OsString>, windows: bool) -> bool {
    env.iter().any(|(name, value)| {
        let name = name.to_string_lossy();
        let named = if windows {
            name.eq_ignore_ascii_case("CODEX_HOME")
        } else {
            name == "CODEX_HOME"
        };
        named && !value.is_empty()
    })
}

#[cfg(test)]
mod tests;
