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

use std::path::PathBuf;

use clauseo_process::Timer;

use crate::cancel::Cancel;
use crate::signin::SigninState;
use crate::signin::codex::identity;
use crate::signin::env::RunKind;
use crate::signin::job::SigninJob;
use crate::signin::{claude, codex, reuse};
use crate::status::{CodexDetect, Identity, claude_detection};

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
    let folder = folder(job)?;
    let (code, parsed) = claude::status_run(job, cancel, RunKind::Detect, &folder).await?;
    let state = claude_detection(code, parsed.as_ref());
    let identity = match (&state, parsed) {
        (SigninState::LoginFound { .. }, Some(parsed)) => Identity {
            email: parsed.email,
            plan: parsed.subscription_type,
            account_id: parsed.account_uuid,
        },
        _ => Identity::default(),
    };
    Some(Detected { state, identity })
}

/// Codex's detection; `None` when it could not tell: a status run that could not, or, for a
/// reusable login, an `account/read` that got no answer (cut short by Cancel or its limit, or
/// an error). Such a read is never "no email": `LoginFound { email: None }` comes only from an
/// answer that names no `ChatGPT` account.
pub(crate) async fn codex<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
) -> Option<Detected> {
    let folder = folder(job)?;
    let code = codex::status_run(job, cancel, RunKind::Detect, &folder).await?;
    let present = job
        .config
        .signin
        .lawyer_home()
        .is_some_and(|lawyer| reuse::lawyer_login_present(&lawyer));
    let verdict = CodexDetect::classify(code, present);
    let identity = if verdict == CodexDetect::Reusable {
        identity::read_as(job, cancel, RunKind::Detect, &folder).await?
    } else {
        Identity::default()
    };
    Some(Detected {
        state: verdict.state(identity.email.clone()),
        identity,
    })
}
