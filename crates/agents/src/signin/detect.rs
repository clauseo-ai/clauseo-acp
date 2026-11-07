//! Detection (the brief, "Detection"): whether the lawyer already has a Claude Code or Codex
//! login, asked of the CLI itself, "with the run environment below minus the home variables and
//! `HOME` change, and without the store override" (`RunKind::Detect`: no `CLAUDE_CONFIG_DIR`,
//! `CLAUDE_SECURESTORAGE_CONFIG_DIR` or `CODEX_HOME`, `HOME` the lawyer's, no
//! `cli_auth_credentials_store`), with "cwd an empty Clauseo folder, so no project settings
//! apply": `<data_dir>/agents/signin/detect`.
//!
//! - Claude: `--cli auth status --json`. "Exit 0 with `authMethod` `claude.ai` is reusable. Any
//!   other signed-in `authMethod` … is a login set up in the lawyer's `~/.claude/settings.json`:
//!   not reusable, `found: ClaudeSettings`" (`status::claude_detection`).
//! - Codex: `login status`, "exit code only, stderr discarded". "Exit 0, then a stat (never an
//!   open)" of the lawyer's login file (`reuse::lawyer_login_present`): present is reusable,
//!   absent is `found: CodexKeyring`. "For a reusable login, `account/read` through `codex
//!   app-server`, same environment, gives the email."
//!
//! Detection never starts reuse: it gives `LoginFound`, and only "Use it" reuses.
//!
//! Neither CLI runs in a lawyer's home that holds nothing of it: the real CLIs write there as
//! they start (Claude Code makes `~/.claude` and `~/.claude.json`, Codex `~/.codex/tmp/arg0`),
//! which would leave Claude Code or Codex files on a machine that never had either. So a stat
//! first ([`lawyer_has_claude`], [`lawyer_has_codex`]): with nothing there, or no lawyer home
//! known, there is no login to reuse, and detection says so with no run: `SignedOut { found:
//! NoLogin }`, what either CLI's status says of an absent login. Codex's rule is its folder
//! rather than `auth.json`, so a keyring login (a `~/.codex` with no `auth.json`) is still found
//! as `CodexKeyring`.

use std::path::{Path, PathBuf};

use clauseo_process::Timer;

use crate::cancel::Cancel;
use crate::signin::codex::identity::{self, AccountRead};
use crate::signin::env::RunKind;
use crate::signin::job::SigninJob;
use crate::signin::{Found, SigninState};
use crate::signin::{claude, codex, reuse};
use crate::status::{CodexDetect, Identity, claude_detection};

/// Claude Code's folder and file in the lawyer's home.
const CLAUDE_FOLDER: &str = ".claude";
const CLAUDE_FILE: &str = ".claude.json";
/// Codex's folder in the lawyer's home.
const CODEX_FOLDER: &str = ".codex";

/// Whether the lawyer's home holds Claude Code's `.claude` or `.claude.json` (a stat, never an
/// open): only then can there be a login to reuse.
pub(crate) fn lawyer_has_claude(lawyer_home: &Path) -> bool {
    [CLAUDE_FOLDER, CLAUDE_FILE]
        .iter()
        .any(|name| std::fs::symlink_metadata(lawyer_home.join(name)).is_ok())
}

/// Whether the lawyer's home holds Codex's `.codex` (a stat): only then can there be a login,
/// in its `auth.json` or, with none there, in the keyring.
pub(crate) fn lawyer_has_codex(lawyer_home: &Path) -> bool {
    std::fs::symlink_metadata(lawyer_home.join(CODEX_FOLDER)).is_ok()
}

/// Detection's answer when there is nothing to reuse, with no run: what a status run that found
/// no login says, for either CLI.
fn nothing_found() -> Detected {
    Detected {
        state: SigninState::SignedOut {
            found: Some(Found::NoLogin),
        },
        identity: Identity::default(),
    }
}

/// What detection found: the state, and for a reusable login the account and plan the CLI's own
/// status gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Detected {
    /// `LoginFound`, or `SignedOut` with what was seen.
    pub(crate) state: SigninState,
    /// The account behind a reusable login (empty otherwise).
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

/// Claude's detection; `None` when it could not tell (no start, killed past 5 s, cancelled).
pub(crate) async fn claude<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
) -> Option<Detected> {
    let lawyer = job.config.signin.lawyer_home();
    if !lawyer.as_deref().is_some_and(lawyer_has_claude) {
        tracing::info!(agent = %job.ticket.agent, "sign-in detection: no Claude Code files in the home folder, no run");
        return Some(nothing_found());
    }
    let folder = folder(job)?;
    let (code, parsed) = claude::status_run(job, cancel, RunKind::Detect, &folder).await?;
    let state = claude_detection(code, parsed.as_ref());
    let identity = match (&state, parsed) {
        (SigninState::LoginFound { .. }, Some(parsed)) => Identity {
            email: parsed.email,
            plan: parsed.subscription_type,
        },
        _ => Identity::default(),
    };
    Some(Detected { state, identity })
}

/// Codex's detection; `None` when it could not tell: a status run that got no answer, or, for a
/// reusable login, an `account/read` that got none (cut short by Cancel or its limit). Such a
/// read is never "no email": `LoginFound { email: None }` comes only from a `ChatGPT` account
/// whose email Codex does not know. A login whose `account/read` names no `ChatGPT` account (an
/// API key, say, `account: null`, or a JSON-RPC error) is not offered: `SignedOut { found: None
/// }`, the plain sign-in card.
pub(crate) async fn codex<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
) -> Option<Detected> {
    let lawyer = job.config.signin.lawyer_home();
    if !lawyer.as_deref().is_some_and(lawyer_has_codex) {
        tracing::info!(agent = %job.ticket.agent, "sign-in detection: no Codex folder in the home folder, no run");
        return Some(nothing_found());
    }
    let folder = folder(job)?;
    let said = codex::status_run(job, cancel, RunKind::Detect, &folder).await;
    if said == codex::StatusSaid::NoAnswer {
        return None;
    }
    let present = lawyer.as_deref().is_some_and(reuse::lawyer_login_present);
    let verdict = CodexDetect::classify(said == codex::StatusSaid::LoggedIn, present);
    let identity = if verdict == CodexDetect::Reusable {
        match identity::read_as(job, cancel, RunKind::Detect, &folder).await {
            AccountRead::Chatgpt(identity) => identity,
            AccountRead::NotChatgpt | AccountRead::NoAccount => {
                return Some(Detected {
                    state: SigninState::SignedOut { found: None },
                    identity: Identity::default(),
                });
            }
            AccountRead::NoAnswer => return None,
        }
    } else {
        Identity::default()
    };
    Some(Detected {
        state: verdict.state(identity.email.clone()),
        identity,
    })
}
