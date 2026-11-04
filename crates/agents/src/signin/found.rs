//! Found, then chosen (the brief, "Found, then chosen", "Reuse stops", "Claude, reused",
//! "Codex, reused" and "Run-time fallback"): the status check every start makes, and the
//! lawyer's four choices about a login they already have.
//!
//! **The status check** (`check_status`, at start and when R20 asks):
//! - a recorded `reused` (for Codex also "a missing file with a home `auth.json`") is checked
//!   first: "At a later start, a recorded `reused` whose login still holds is `Reused` with no
//!   question." Claude holds when detection still finds a `claude.ai` login with the accepted
//!   email (else `ReusedChanged`, or `ReusedSignedOut` when it is signed out) and the status
//!   under the session's environment is signed in (else the run-time fallback). Codex holds while
//!   the two names are one file (`reuse::codex_link_holds`; else `CodexLinkSplit`, or
//!   `ReusedSignedOut` when the lawyer's file is gone).
//! - otherwise Clauseo's own login: "A Clauseo-owned login, once made, wins over reuse."
//! - otherwise, unless `no-reuse` is there, detection (`super::detect`): `LoginFound`, or
//!   `SignedOut` with what was seen.
//!
//! **Reuse stops** ([`stop`]) with the reason in `found`, and never with a logout: "With no
//! reason … it writes `no-reuse`, so the next start does not reuse again without asking. With a
//! reason it clears the reuse record only, so the next start shows `LoginFound` again." For Codex
//! the home's link goes too (`reuse::unlink_reused_login`), the leave order's unlink step.

use clauseo_process::Timer;

use crate::signin::detect::{self, Detected};
use crate::signin::env::{RunKind, Vendor};
use crate::signin::job::SigninJob;
use crate::signin::marker::{self, LoginSource};
use crate::signin::{
    Found, Method, PasteCode, SigninCode, SigninNote, SigninState, Source, claude, codex, reuse,
};

/// Whether the agent's login is the lawyer's, reused, by Clauseo's record: `login-source` says
/// `reused`, or for Codex there is no marker and the home holds a login file. No logout runs
/// then.
pub(crate) fn is_reused<T>(job: &SigninJob<T>) -> bool {
    let source = marker::read_source(&job.config.data_dir, &job.ticket.agent);
    match job.vendor {
        Vendor::Claude => matches!(source, Some(LoginSource::Reused { .. })),
        Vendor::Codex => reuse::codex_reused(source.as_ref(), &job.home),
    }
}

/// `SignedIn { source: Reused }` for `email` and `plan`, now.
fn reused_signed_in<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    email: Option<String>,
    plan: Option<String>,
) -> SigninState {
    SigninState::SignedIn {
        email,
        plan,
        source: Source::Reused,
        at: job.now(),
    }
}

/// Stops reuse with `why` and gives `SignedOut { found: why }`; never a logout. No reason:
/// `no-reuse` is written. For Codex, a reused home's link is removed. Then the reuse record is
/// cleared. `signin.failed` when a marker cannot be written, since the next start could then
/// reuse again without asking.
pub(crate) fn stop<T: Timer + Send + Sync>(job: &SigninJob<T>, why: Option<Found>) -> SigninState {
    let data = &job.config.data_dir;
    let agent = &job.ticket.agent;
    if why.is_none() && marker::write_no_reuse(data, agent).is_err() {
        return job.failed(SigninCode::Failed);
    }
    if job.vendor == Vendor::Codex {
        let source = marker::read_source(data, agent);
        if reuse::codex_reused(source.as_ref(), &job.home)
            && reuse::unlink_reused_login(&job.home).is_err()
        {
            return job.failed(SigninCode::Failed);
        }
    }
    if marker::clear_source(data, agent).is_err() {
        return job.failed(SigninCode::Failed);
    }
    SigninState::SignedOut { found: why }
}

/// The status check: see the module docs. `None` leaves the state as it is (a run could not
/// tell).
pub(crate) async fn check<T: Timer + Send + Sync>(job: &SigninJob<T>) -> Option<SigninState> {
    match job.vendor {
        Vendor::Claude => check_claude(job).await,
        Vendor::Codex => check_codex(job).await,
    }
}

async fn check_claude<T: Timer + Send + Sync>(job: &SigninJob<T>) -> Option<SigninState> {
    let cancel = &job.ticket.cancel;
    if let Some(LoginSource::Reused { email }) =
        marker::read_source(&job.config.data_dir, &job.ticket.agent)
    {
        let found = detect::claude(job, cancel).await?;
        return match &found.state {
            SigninState::LoginFound { email: now, .. } if *now == email => {
                claude_session(job, email, found, false).await
            }
            SigninState::LoginFound { .. } => Some(stop(job, Some(Found::ReusedChanged))),
            _ => Some(stop(job, Some(Found::ReusedSignedOut))),
        };
    }
    if let Some(identity) = claude::status(job, cancel).await? {
        return Some(claude::signed_in(job, identity));
    }
    if marker::no_reuse(&job.config.data_dir, &job.ticket.agent) {
        return Some(SigninState::SignedOut { found: None });
    }
    Some(detect::claude(job, cancel).await?.state)
}

/// Claude's status under the session's environment (`CLAUDE_CONFIG_DIR` Clauseo's folder,
/// `CLAUDE_SECURESTORAGE_CONFIG_DIR=""`): "Before the first session, status under that env must
/// report `loggedIn: true`." Signed in: `Reused` (with `record`, `login-source` is written
/// first). Signed out while detection found the login, the run-time fallback: "S18 logs
/// `claude_reuse_fallback` and offers Clauseo's own sign-in" (the note, then reuse stops with no
/// reason, so it is not offered again until "Look for my Claude Code login").
async fn claude_session<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    email: Option<String>,
    found: Detected,
    record: bool,
) -> Option<SigninState> {
    let reused = RunKind::Reused { home: &job.home };
    let Some(session) = claude::status_as(job, &job.ticket.cancel, reused).await? else {
        job.note(SigninNote::ClaudeReuseFallback);
        return Some(stop(job, None));
    };
    if record {
        // Cancelled or replaced while status ran: no record of a reuse nobody chose to keep.
        if !job.shared.signin.is_live(&job.ticket) {
            return Some(job.failed(SigninCode::Cancelled));
        }
        let source = LoginSource::Reused {
            email: email.clone(),
        };
        if marker::write_source(&job.config.data_dir, &job.ticket.agent, &source).is_err() {
            return Some(job.failed(SigninCode::Failed));
        }
    }
    let plan = session.plan.or(found.identity.plan);
    Some(reused_signed_in(job, email, plan))
}

async fn check_codex<T: Timer + Send + Sync>(job: &SigninJob<T>) -> Option<SigninState> {
    let cancel = &job.ticket.cancel;
    let source = marker::read_source(&job.config.data_dir, &job.ticket.agent);
    if reuse::codex_reused(source.as_ref(), &job.home) {
        let Some(lawyer) = job.config.signin.lawyer_home() else {
            return Some(stop(job, Some(Found::CodexLinkSplit)));
        };
        if let Some(why) = reuse::codex_link_holds(&job.home, &lawyer).found() {
            return Some(stop(job, Some(why)));
        }
        let reused = RunKind::Reused { home: &job.home };
        let identity = codex::identity::read_as(job, cancel, reused, &job.home)
            .await
            .unwrap_or_default();
        let accepted = match source {
            Some(LoginSource::Reused { email }) => email,
            _ => None,
        };
        return Some(reused_signed_in(
            job,
            identity.email.or(accepted),
            identity.plan,
        ));
    }
    if codex::status(job, cancel).await? {
        let identity = codex::identity::read(job, cancel).await;
        return Some(codex::signed_in(job, identity));
    }
    if marker::no_reuse(&job.config.data_dir, &job.ticket.agent) {
        return Some(SigninState::SignedOut { found: None });
    }
    Some(detect::codex(job, cancel).await?.state)
}

/// `find_login` (UD2's "Look for my … login"): "deletes `no-reuse` and detects again", through
/// the status check.
pub(crate) async fn find_login<T: Timer + Send + Sync>(job: &SigninJob<T>) -> Option<SigninState> {
    if marker::remove_no_reuse(&job.config.data_dir, &job.ticket.agent).is_err() {
        return Some(job.failed(SigninCode::Failed));
    }
    check(job).await
}

/// `use_found_login` (the lawyer's "Use it"): "R20 writes Claude's env, or S18 makes Codex's
/// link; then `login-source` `reused` with the accepted email; then `SignedIn { source: Reused
/// }`." Detection runs again first, and "Use it" goes on only when it names an email and that
/// email is the one the lawyer was shown. Otherwise nothing is linked or recorded and the new
/// detection is the state (`LoginFound` again, or what was seen): an account other than the one
/// shown, or one with no email, is never reused on a choice the lawyer made about another.
/// Detection that could not tell ends the job: `signin.cancelled` when it was cancelled (or the
/// job replaced), else `signin.failed`.
pub(crate) async fn use_found<T: Timer + Send + Sync>(job: &SigninJob<T>) -> SigninState {
    let cancel = &job.ticket.cancel;
    let shown = match job.shared.signin.state(&job.ticket.agent) {
        Some(SigninState::LoginFound { email, .. }) => email,
        _ => None,
    };
    let found = match job.vendor {
        Vendor::Claude => detect::claude(job, cancel).await,
        Vendor::Codex => detect::codex(job, cancel).await,
    };
    if !job.shared.signin.is_live(&job.ticket) {
        return job.failed(SigninCode::Cancelled);
    }
    let Some(found) = found else {
        return job.failed(SigninCode::Failed);
    };
    let email = match &found.state {
        SigninState::LoginFound {
            email: Some(email), ..
        } if shown.as_deref() == Some(email.as_str()) => email.clone(),
        other => return other.clone(),
    };
    match job.vendor {
        Vendor::Claude => claude_session(job, Some(email), found, true)
            .await
            .unwrap_or_else(|| {
                if cancel.is_cancelled() {
                    job.failed(SigninCode::Cancelled)
                } else {
                    job.failed(SigninCode::Failed)
                }
            }),
        Vendor::Codex => use_codex(job, email, found).await,
    }
}

/// How Codex's link step ended.
enum Linked {
    /// The link and the `reused` record are made.
    Done,
    /// The job was cancelled or replaced: nothing is left linked or recorded.
    Stopped,
    /// The link could not be made: the fallback runs.
    LinkFailed,
    /// The record could not be written: the link was removed again.
    RecordFailed,
}

/// Codex's "Use it": the link (`reuse::link_reused_login`), then the record. "Another volume, or
/// any link error: Clauseo's own sign-in, logged" (the `codex_link_failed` note, then the
/// browser sign-in).
async fn use_codex<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    email: String,
    found: Detected,
) -> SigninState {
    match link_and_record(job, &email).await {
        Linked::Done => reused_signed_in(job, Some(email), found.identity.plan),
        Linked::Stopped => job.failed(SigninCode::Cancelled),
        Linked::RecordFailed => job.failed(SigninCode::Failed),
        Linked::LinkFailed => {
            // The lock is let go by now: the browser sign-in takes it for its own run.
            job.note(SigninNote::CodexLinkFailed);
            codex::sign_in(job, Method::Browser).await
        }
    }
}

/// The link and the record, under the lock on Codex's login files. The job must still be the
/// agent's and not cancelled right before the link and again right before the record. Cancelled
/// or replaced before the link: nothing was done, nothing is undone. After it: Clauseo's own
/// name goes again (`remove_file` of the home's name, never the lawyer's), and no `reused` is
/// written. A newer job's Codex login or `use_different_account` waits for this lock, so it
/// never runs while this step could still leave the files linked.
async fn link_and_record<T: Timer + Send + Sync>(job: &SigninJob<T>, email: &str) -> Linked {
    let _files = job.shared.signin.codex_files().await;
    let hub = &job.shared.signin;
    if !hub.is_live(&job.ticket) {
        return Linked::Stopped;
    }
    let linked = job
        .config
        .signin
        .lawyer_home()
        .ok_or_else(|| std::io::Error::other("no lawyer's home folder"))
        .and_then(|lawyer| reuse::link_reused_login(&job.config.data_dir, &job.home, &lawyer));
    if linked.is_err() {
        return Linked::LinkFailed;
    }
    if !hub.is_live(&job.ticket) {
        drop(reuse::unlink_reused_login(&job.home));
        return Linked::Stopped;
    }
    let source = LoginSource::Reused {
        email: Some(email.to_owned()),
    };
    if marker::write_source(&job.config.data_dir, &job.ticket.agent, &source).is_err() {
        // The link alone would read as reused; it goes, and nothing is reused.
        drop(reuse::unlink_reused_login(&job.home));
        return Linked::RecordFailed;
    }
    Linked::Done
}

/// `use_different_account` ("Sign in with a different account"): "writes `no-reuse` and starts
/// Clauseo's own sign-in; the CLI's login is untouched." Claude's `--claudeai` login, or Codex's
/// browser one (device code is offered, never started first).
///
/// For Codex the order is explicit, so `codex login` never runs while the home's login file is
/// the lawyer's: (1) taking the job's slot set the cancel of any running "Use it" and made it no
/// longer the agent's; (2) [`clear_replaced_reuse`] waits for the lock on Codex's login files,
/// so that "Use it" has either linked nothing or finished, and removes any reused name it left;
/// (3) the login takes the lock again and runs its guard, as every Codex login does.
pub(crate) async fn use_different<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    paste: Option<async_channel::Receiver<PasteCode>>,
) -> SigninState {
    if marker::write_no_reuse(&job.config.data_dir, &job.ticket.agent).is_err() {
        return job.failed(SigninCode::Failed);
    }
    match job.vendor {
        Vendor::Claude => claude::sign_in(job, false, paste).await,
        Vendor::Codex => {
            if let Some(stopped) = clear_replaced_reuse(job).await {
                return stopped;
            }
            codex::sign_in(job, Method::Browser).await
        }
    }
}

/// Under the lock on Codex's login files: a reused name in the home (a "Use it" that recorded
/// just before this job replaced it) goes, Clauseo's own name only, with its record. The call
/// is refused while the state is `SignedIn { Reused }`, so any such name is one the lawyer
/// chose against. `Some` ends the job: cancelled or replaced meanwhile, or a removal failed.
async fn clear_replaced_reuse<T: Timer + Send + Sync>(job: &SigninJob<T>) -> Option<SigninState> {
    let _files = job.shared.signin.codex_files().await;
    if !job.shared.signin.is_live(&job.ticket) {
        return Some(job.failed(SigninCode::Cancelled));
    }
    let data = &job.config.data_dir;
    let agent = &job.ticket.agent;
    let source = marker::read_source(data, agent);
    let reused =
        source != Some(LoginSource::Own) && reuse::codex_reused(source.as_ref(), &job.home);
    if reused
        && (reuse::unlink_reused_login(&job.home).is_err()
            || marker::clear_source(data, agent).is_err())
    {
        return Some(job.failed(SigninCode::Failed));
    }
    None
}

/// `stop_using_login(why)`: [`stop`], unless the record says the login is Clauseo's own (then
/// nothing changes: that one signs out with `sign_out`).
pub(crate) fn stop_using<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    why: Option<Found>,
) -> Option<SigninState> {
    let source = marker::read_source(&job.config.data_dir, &job.ticket.agent);
    if source == Some(LoginSource::Own) {
        return None;
    }
    Some(stop(job, why))
}
