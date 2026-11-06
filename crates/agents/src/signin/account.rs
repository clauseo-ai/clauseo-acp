//! The account behind an agent's login, read for R17's identity check (decision record 0048,
//! "identity check (R17)"): one status run under the environment a session of that login sees,
//! with no change to the sign-in state, and the account it names as an [`AccountHash`].
//!
//! - Claude: `auth status --json` with the session's environment ([`RunKind::Own`] for
//!   Clauseo's own login, [`RunKind::Reused`] for the lawyer's), its `email`.
//! - Codex: `login status` through the install's Codex CLI ([`crate::state::Installed::codex_cli`];
//!   exit 0 with its signed-in line is signed in), then `account/read` through the app-server,
//!   its `ChatGPT` `email`. Exit 0 with no signed-in line, no answer, a missing CLI, or a signed-in
//!   status whose `account/read` names no account or errs is [`AccountRead::Failed`].
//!
//! The identity comes only from that output: nothing here opens a credential. The email is
//! hashed at once ([`AccountHash::of`]) and never kept, logged or shown. A read's runs keep their
//! own limits (Claude's status 5 s; Codex's `login status` 15 s and its app-server 20 s); the
//! caller bounds the whole.

use std::future::Future;
use std::pin::Pin;

use clauseo_process::Timer;
use sha2::{Digest as _, Sha256};

use crate::integrity::to_hex;
use crate::signin::env::{RunKind, Vendor};
use crate::signin::job::SigninJob;
use crate::signin::codex::{StatusSaid, identity};
use crate::signin::{claude, codex, found};

/// The SHA-256, lowercase hex, of an account's normalised identity (its email or account id,
/// trimmed and lower-cased). Clauseo records and compares this, never the email. Its `Debug`
/// prints no digit of it.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AccountHash(String);

impl AccountHash {
    /// The hash of `identity`, trimmed and lower-cased; `None` when nothing is left.
    pub fn of(identity: &str) -> Option<AccountHash> {
        let normal = identity.trim().to_lowercase();
        if normal.is_empty() {
            return None;
        }
        Some(AccountHash(to_hex(&Sha256::digest(normal.as_bytes()))))
    }

    /// A hash read back from its hex, as [`AccountHash::as_hex`] wrote it: 64 lowercase hex
    /// digits, else `None`.
    pub fn from_hex(hex: &str) -> Option<AccountHash> {
        let digits = hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        digits.then(|| AccountHash(hex.to_owned()))
    }

    /// Its lowercase hex.
    pub fn as_hex(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for AccountHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccountHash(..)")
    }
}

/// What one read of an agent's account found.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccountRead {
    /// The status named this account.
    Account(AccountHash),
    /// The status answered and named no account: signed out, or a login with no email (an API
    /// key, a Console login without one).
    NoAccount,
    /// The status could not tell: no start, no answer within its limit, cancelled, or the
    /// read was refused (no install, no home).
    Failed,
}

/// A read's answer, on its way from the job's thread.
pub type AccountFuture = Pin<Box<dyn Future<Output = AccountRead> + Send>>;

/// The read: see the module docs.
pub(crate) async fn read<T: Timer + Send + Sync>(job: &SigninJob<T>) -> AccountRead {
    let cancel = &job.ticket.cancel;
    let kind = if found::is_reused(job) {
        RunKind::Reused { home: &job.home }
    } else {
        RunKind::Own { home: &job.home }
    };
    match job.vendor {
        Vendor::Claude => match claude::status_as(job, cancel, kind).await {
            None => AccountRead::Failed,
            Some(None) => AccountRead::NoAccount,
            Some(Some(identity)) => named(identity.email.as_deref()),
        },
        // Codex's CLI missing from the install: nothing can be asked, so nothing is known.
        Vendor::Codex if job.script().is_none() => AccountRead::Failed,
        Vendor::Codex => match codex::status_run(job, cancel, kind, &job.home).await {
            StatusSaid::SignedOut => AccountRead::NoAccount,
            // Exit 0 with no signed-in line is not Codex's CLI answering: never signed in, and
            // the account is not known either way.
            StatusSaid::NoMarker | StatusSaid::NoAnswer => AccountRead::Failed,
            StatusSaid::LoggedIn => match codex::identity::read_as(job, cancel, kind, &job.home).await {
                identity::AccountRead::Chatgpt(identity) => named(identity.email.as_deref()),
                // An API-key, Bedrock or unknown kind of account names no email.
                identity::AccountRead::NotChatgpt => AccountRead::NoAccount,
                // `login status` said signed in, yet `account/read` named no account or answered
                // with an error: who is signed in is not known.
                identity::AccountRead::NoAccount | identity::AccountRead::NoAnswer => {
                    AccountRead::Failed
                }
            },
        },
    }
}

/// [`AccountRead::Account`] for an email the status gave, else [`AccountRead::NoAccount`].
fn named(email: Option<&str>) -> AccountRead {
    email
        .and_then(AccountHash::of)
        .map_or(AccountRead::NoAccount, AccountRead::Account)
}

#[cfg(test)]
mod tests;
