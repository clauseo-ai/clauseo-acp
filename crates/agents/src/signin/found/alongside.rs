//! Claude's status check with detection run alongside the status of Clauseo's own login, when
//! that is safe ([`detect_alongside`]), and the one choice both orders share ([`pick_claude`]).

use clauseo_process::Timer;

use crate::signin::detect::Detected;
use crate::signin::job::SigninJob;
use crate::signin::marker::{self, LoginSource};
use crate::signin::{Found, SigninState, claude};

/// Whether Claude's status check runs detection alongside the status of Clauseo's own login
/// rather than after it: only when nothing is recorded (no `login-source`, so neither a
/// login of Clauseo's own nor a reuse; no `no-reuse`), and never on macOS. Then the own
/// login is most likely signed out and detection would run next anyway; running both at once
/// saves detection's time before the card shows Sign in or the login found.
///
/// The safety rule: detection may never cause a prompt or a write the lawyer did not ask for.
/// - Claude Code's `auth status` reads its stored tokens (`hasToken`, `loginExpiresAt`),
///   which on macOS sit in the Keychain item `Claude Code-credentials`, read through
///   `security find-generic-password`: a read that may prompt. So macOS stays serial.
/// - `auth status` also writes the lawyer's `~/.claude.json` (and a backup) the first time a
///   CLI version meets it, creating it when missing. So detection runs early only where the
///   serial check would most likely run it too: with a login of Clauseo's own recorded, its
///   result would be thrown away, so it waits for the own status as before.
pub(super) fn detect_alongside(source: Option<&LoginSource>, no_reuse: bool) -> bool {
    !cfg!(target_os = "macos") && source.is_none() && !no_reuse
}

/// Claude's choice between the status of Clauseo's own login, which answered (`own`: the
/// identity when signed in; one that could not tell ended the check before this), and detection
/// (`found`, awaited only when needed), the same whether detection ran alongside or after: the
/// own login signed in wins, and detection's answer is dropped; signed out, `ReuseOff` while
/// `no-reuse` is there, else detection's answer.
pub(super) async fn pick_claude<T, F, Fut>(
    job: &SigninJob<T>,
    own: Option<crate::status::Identity>,
    found: F,
) -> Option<SigninState>
where
    T: Timer + Send + Sync,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Option<Detected>>,
{
    if let Some(identity) = own {
        return Some(claude::signed_in(job, identity));
    }
    if marker::no_reuse(&job.config.data_dir, &job.ticket.agent) {
        return Some(SigninState::SignedOut {
            found: Some(Found::ReuseOff),
        });
    }
    Some(found().await?.state)
}
