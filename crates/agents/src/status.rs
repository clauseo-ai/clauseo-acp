//! Reading the vendors' own status: Claude's `auth status --json`, Codex's `login status` exit
//! code and `account/read` result, and the error text that means "sign in again". Pure
//! functions; the runs that produce their input are the sign-in flows'.
//!
//! The email and plan come only from here, never from a token file.

use clauseo_codex_appserver::{Account, GetAccountResponse};
use serde::Deserialize;

use crate::signin::{Found, LoginFrom, SigninState};

/// The `authMethod` of a Claude.ai plan login, the one kind Clauseo reuses.
const CLAUDE_AI: &str = "claude.ai";

/// The `apiProvider` of Anthropic's own API.
const FIRST_PARTY: &str = "firstParty";

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
    /// `apiProvider`: `firstParty`, or a cloud provider set up in settings.
    #[serde(default)]
    pub(crate) api_provider: Option<String>,
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

/// Detection's verdict on Claude's status (the brief, "Detection"): exit 0 with `authMethod`
/// `claude.ai` is reusable, giving `LoginFound { from: ClaudeCode, email }`; "any other
/// signed-in `authMethod` (`third_party`, `api_key_helper`, `api_key`, `oauth_token`) is a
/// login set up in the lawyer's `~/.claude/settings.json`: not reusable, `found:
/// ClaudeSettings`". A `claude.ai` login whose `apiProvider` is set and not `firstParty` reads as
/// set up in settings too. Anything else, a non-zero exit or output that does not parse
/// included, is `SignedOut` with nothing found.
pub(crate) fn claude_detection(exit: Option<i32>, status: Option<&ClaudeStatus>) -> SigninState {
    let signed_out = |found| SigninState::SignedOut { found };
    let Some(status) = status.filter(|status| exit == Some(0) && status.logged_in) else {
        return signed_out(None);
    };
    match status.auth_method.as_deref() {
        None | Some(NONE) => signed_out(None),
        Some(CLAUDE_AI) if first_party(status) => SigninState::LoginFound {
            from: LoginFrom::ClaudeCode,
            email: status.email.clone(),
        },
        Some(_) => signed_out(Some(Found::ClaudeSettings)),
    }
}

/// Whether the status names Anthropic's own API, or no provider at all.
fn first_party(status: &ClaudeStatus) -> bool {
    status
        .api_provider
        .as_deref()
        .is_none_or(|provider| provider == FIRST_PARTY)
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

/// Whether `codex login status` says signed in: exit 0. Its output is never read (stderr
/// prints a masked key prefix).
pub(crate) fn codex_logged_in(exit: Option<i32>) -> bool {
    exit == Some(0)
}

/// Detection's verdict on Codex, before `account/read`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexDetect {
    /// `login status` is not 0: nothing found.
    SignedOut,
    /// Exit 0 and the lawyer's login file is there: a file-mode login, reusable.
    Reusable,
    /// Exit 0 and no file: a keyring or Windows secrets login, not reusable.
    Keyring,
}

impl CodexDetect {
    /// "Exit 0, then a stat (never an open) of `~/.codex/auth.json`: present is a file-mode
    /// login, reusable; absent is a keyring or Windows secrets login, not reusable". `file_present`
    /// is that stat's answer.
    pub(crate) fn classify(exit: Option<i32>, file_present: bool) -> CodexDetect {
        match (codex_logged_in(exit), file_present) {
            (false, _) => CodexDetect::SignedOut,
            (true, true) => CodexDetect::Reusable,
            (true, false) => CodexDetect::Keyring,
        }
    }

    /// The state: `LoginFound { from: Codex, email }` for a reusable login (`email` from
    /// `account/read`), `SignedOut { found: CodexKeyring }` for a keyring one, else `SignedOut`.
    pub(crate) fn state(self, email: Option<String>) -> SigninState {
        match self {
            CodexDetect::SignedOut => SigninState::SignedOut { found: None },
            CodexDetect::Reusable => SigninState::LoginFound {
                from: LoginFrom::Codex,
                email,
            },
            CodexDetect::Keyring => SigninState::SignedOut {
                found: Some(Found::CodexKeyring),
            },
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
