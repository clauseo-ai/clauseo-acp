//! Reading the vendors' own status: Claude's `auth status --json`, Codex's `login status` exit
//! code and signed-in line and `account/read` result, and the error text that means "sign in
//! again". Pure functions; the runs that produce their input are the sign-in flows'.
//!
//! The email and plan come only from here, never from a token file.

use clauseo_codex_appserver::{Account, GetAccountResponse};
use serde::Deserialize;

use crate::signin::{Found, LoginFrom, SigninState};

/// The `authMethod` of no login.
const NONE: &str = "none";

/// What Claude's `auth status --json` says, with unknown fields ignored. `email` and
/// `subscriptionType` are the brief's defaults until G7 row 14 records the real names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudeStatus {
    /// `loggedIn`; absent reads as false.
    #[serde(default)]
    pub(crate) logged_in: bool,
    /// `authMethod`: `claude.ai`, `oauth_token`, `api_key_helper`, `api_key`, `third_party` or
    /// `none` (research 42).
    #[serde(default)]
    pub(crate) auth_method: Option<String>,
    /// `email`.
    #[serde(default)]
    pub(crate) email: Option<String>,
    /// `subscriptionType`: the raw plan string.
    #[serde(default)]
    pub(crate) subscription_type: Option<String>,
}

/// The account and plan a status reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Identity {
    /// The email, when reported.
    pub(crate) email: Option<String>,
    /// The raw plan string, when reported.
    pub(crate) plan: Option<String>,
}

/// Claude's status JSON from its stdout: the whole text, or else its last line that parses
/// (a CLI may print a notice first). `None` when nothing parses.
pub(crate) fn parse_claude_status(stdout: &str) -> Option<ClaudeStatus> {
    serde_json::from_str(stdout.trim()).ok().or_else(|| {
        stdout
            .lines()
            .rev()
            .map(str::trim)
            .filter(|line| line.starts_with('{'))
            .find_map(|line| serde_json::from_str(line).ok())
    })
}

/// Detection's verdict on Claude, from a stat alone: with the lawyer's
/// `~/.claude/.credentials.json` a regular file, `LoginFound { from: ClaudeCode, email: None }`
/// (detection starts no CLI, so it knows no email); else `SignedOut { found: NoLogin }`.
/// `Found::ClaudeSettings` is no longer told apart: telling needs Claude Code itself.
pub(crate) fn claude_detection(file_present: bool) -> SigninState {
    if file_present {
        SigninState::LoginFound {
            from: LoginFrom::ClaudeCode,
            email: None,
        }
    } else {
        SigninState::SignedOut {
            found: Some(Found::NoLogin),
        }
    }
}

/// The account behind a Claude status in Clauseo's own folder, once signed in: exit 0 and
/// `loggedIn: true`, whatever the `authMethod` (Clauseo's folder holds no settings that could
/// make another login; `--console` gives a Console one). `None` while signed out.
pub(crate) fn claude_signed_in(
    exit: Option<i32>,
    status: Option<&ClaudeStatus>,
) -> Option<Identity> {
    let status = status.filter(|status| exit == Some(0) && status.logged_in)?;
    if status.auth_method.as_deref() == Some(NONE) {
        return None;
    }
    Some(Identity {
        email: status.email.clone(),
        plan: status.subscription_type.clone(),
    })
}

/// How Codex's `login status` begins the line that says it is signed in, on stderr: `Logged in
/// using ChatGPT`, or `Logged in using an API key - <masked prefix>` (openai/codex
/// `rust-v0.162.1`, `codex-rs/cli/src/login.rs`). Signed out it prints `Not logged in` and
/// exits 1.
pub(crate) const CODEX_LOGGED_IN: &str = "Logged in using ";

/// Whether one output line of `codex login status` is its signed-in line. The line itself is
/// never kept or logged: the API-key one carries a masked key prefix.
pub(crate) fn codex_says_logged_in(line: &str) -> bool {
    line.trim_start().starts_with(CODEX_LOGGED_IN)
}

/// Whether `codex login status` says signed in: exit 0 **and** its signed-in line
/// ([`codex_says_logged_in`]). Exit 0 alone is not enough: codex-acp's adapter, run by mistake
/// in the CLI's place, exits 0 for any arguments and prints nothing.
pub(crate) fn codex_logged_in(exit: Option<i32>, said_logged_in: bool) -> bool {
    exit == Some(0) && said_logged_in
}

/// Detection's verdict on Codex, from a stat alone: with the lawyer's login file a regular
/// file there, `LoginFound { from: Codex, email: None }` (no email: detection reads neither the
/// file nor Codex's `account/read`, and `login status` writes `~/.codex/tmp`; the reused
/// confirm after "Use it" gives the email and plan); else `SignedOut { found: NoLogin }`.
/// `Found::CodexKeyring` is no longer told apart: telling needs Codex's CLI.
pub(crate) fn codex_detection(file_present: bool) -> SigninState {
    if file_present {
        SigninState::LoginFound {
            from: LoginFrom::Codex,
            email: None,
        }
    } else {
        SigninState::SignedOut {
            found: Some(Found::NoLogin),
        }
    }
}

/// The account behind `account/read`: a `ChatGPT` sign-in's `email` and `plan_type`. `None` for
/// no account or another kind (an API key, Bedrock), which is not a plan login.
pub(crate) fn codex_identity(response: &GetAccountResponse) -> Option<Identity> {
    match response.account.as_ref()? {
        Account::Chatgpt(account) => Some(Identity {
            email: account.email.clone(),
            plan: Some(account.plan_type.as_str().to_owned()),
        }),
        Account::ApiKey | Account::AmazonBedrock(_) | Account::Unknown(_) => None,
    }
}

/// The error codes Codex gives when its refresh token no longer works, so only a new sign-in
/// helps: `refresh_token_reused` (the race the brief names: the lawyer's CLI and Clauseo
/// refreshed at once), and the expired and revoked cases beside it.
const SIGN_IN_AGAIN: [&str; 3] = [
    "refresh_token_reused",
    "refresh_token_expired",
    "refresh_token_invalidated",
];

/// Whether an agent's error text means the lawyer must sign in again: it names one of Codex's
/// refresh-token failures (`refresh_token_reused`, `refresh_token_expired`,
/// `refresh_token_invalidated`), in any case. R20 maps a turn failing this way to
/// `agent.auth_required`; "Sign in again" then re-detects first.
pub fn is_sign_in_again(error_text: &str) -> bool {
    let text = error_text.to_ascii_lowercase();
    SIGN_IN_AGAIN.iter().any(|code| text.contains(code))
}

#[cfg(test)]
mod tests;
