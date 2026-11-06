//! The account behind an agent's login, read for R17's identity check (decision record 0048,
//! "identity check (R17)"): one status run under the environment a session of that login sees,
//! with no change to the sign-in state, and the account it names as an [`AccountHash`].
//!
//! - Claude: `auth status --json` with the session's environment ([`RunKind::Own`] for
//!   Clauseo's own login, [`RunKind::Reused`] for the lawyer's), its `email`, else its
//!   `accountUuid` (an id is enough).
//! - Codex: `login status` (exit 0 is signed in), then `account/read` through the app-server,
//!   its `ChatGPT` `email` (the pinned schema's account has no id).
//!
//! The identity comes only from that output: nothing here opens a credential. The email is
//! hashed at once ([`AccountHash::of`]) and never kept, logged or shown. A read's runs keep their
//! own limits (5 s for each status run, 10 s for the app-server); the caller bounds the whole.

use std::future::Future;
use std::pin::Pin;

use clauseo_process::Timer;
use sha2::{Digest as _, Sha256};

use crate::integrity::to_hex;
use crate::signin::env::{RunKind, Vendor};
use crate::signin::job::SigninJob;
use crate::signin::{claude, codex, found};
use crate::status::{Identity, codex_logged_in};

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
            Some(Some(identity)) => named(&identity),
        },
        Vendor::Codex => match codex::status_run(job, cancel, kind, &job.home).await {
            None => AccountRead::Failed,
            Some(code) if !codex_logged_in(code) => AccountRead::NoAccount,
            Some(_) => match codex::identity::read_as(job, cancel, kind, &job.home).await {
                None => AccountRead::Failed,
                Some(identity) => named(&identity),
            },
        },
    }
}

/// [`AccountRead::Account`] for the email the status gave, else for its account id; else
/// [`AccountRead::NoAccount`].
fn named(identity: &Identity) -> AccountRead {
    let email = identity.email.as_deref().and_then(AccountHash::of);
    email
        .or_else(|| identity.account_id.as_deref().and_then(AccountHash::of))
        .map_or(AccountRead::NoAccount, AccountRead::Account)
}

#[cfg(test)]
mod tests;
