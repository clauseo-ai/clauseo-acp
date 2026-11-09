//! Found, then chosen (the brief, "Found, then chosen", "Reuse stops", "Claude, reused",
//! "Codex, reused" and "Run-time fallback"): the status check every start makes, and the
//! lawyer's four choices about a login they already have.
//!
//! **The status check** (`check_status`, at start and when R20 asks):
//! - a recorded `reused` (for Codex also "a missing file with a home `auth.json`") is checked
//!   first: "At a later start, a recorded `reused` whose login still holds is `Reused` with no
//!   question." Claude holds when detection still finds a login (a stat; else
//!   `ReusedSignedOut`; a recorded email is compared only with one detection gives, which it
//!   never does, so `ReusedChanged` is not produced) and the status
//!   under the session's environment is signed in (else the run-time fallback). Codex holds while
//!   the two names are one file (`reuse::codex_link_holds`; else `CodexLinkSplit`, or
//!   `ReusedSignedOut` when the lawyer's file is gone) and Codex's own status, through its CLI,
//!   confirms the account (`codex::confirm`; signed out is `ReusedSignedOut` too).
//! - Codex's status that gets no answer at all (a run killed at its 15 s or 20 s limit, or one
//!   that did not start) is asked once more ([`confirm_twice`]); no answer again, the state is
//!   left exactly as it was: never `Failed`, never downgraded (a reuse is not ended, its link
//!   stays, and nothing restarts), never upgraded, and a `state_kept` note is logged. A definite
//!   negative answer signs out at once: "Not logged in", exit 0 with no signed-in line,
//!   `account: null`, a JSON-RPC error, or an account that is not a `ChatGPT` one.
//! - otherwise Clauseo's own login: "A Clauseo-owned login, once made, wins over reuse."
//! - otherwise, unless `no-reuse` is there, detection (`super::detect`): `LoginFound`, or
//!   `SignedOut` with what was seen. Detection is a stat for both agents, so `LoginFound` has
//!   no email; "Use it" confirms through the agent's own status under the reused environment.
//!   R20's session-start identity check (#554) enforces the account.
//!
//! **Reuse stops** ([`stop`]) with the reason in `found`, and never with a logout: "With no
//! reason … it writes `no-reuse`, so the next start does not reuse again without asking. With a
//! reason it clears the reuse record only, so the next start shows `LoginFound` again." For Codex
//! the home's link goes too (`reuse::unlink_reused_login`), the leave order's unlink step.

use std::pin::pin;
use std::time::Duration;

use clauseo_process::Timer;
use futures::future::{Either, select};

use crate::signin::codex::Confirmation;
use crate::signin::detect::{self, Detected};
use crate::signin::env::{RunKind, Vendor};
use crate::signin::job::SigninJob;
use crate::signin::marker::{self, LoginSource};
use crate::signin::{
    Found, Method, PasteCode, SigninCode, SigninNote, SigninState, Source, Unconfirmed, claude,
    codex, reuse,
};
use crate::status::Identity;

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

/// Stops reuse with `why` and gives `SignedOut { found: why }`; never a logout. No reason
/// (or one that is no stop reason, [`Found::is_stop_reason`]): `no-reuse` is written, and the
/// state is `SignedOut { found: ReuseOff }`. For Codex, a reused home's link is removed. Then
/// the reuse record is cleared. `signin.failed` when a marker cannot be written, since the next
/// start could then reuse again without asking.
pub(crate) async fn stop<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    why: Option<Found>,
) -> SigninState {
    let why = why.filter(|why| why.is_stop_reason());
    let data = &job.config.data_dir;
    let agent = &job.ticket.agent;
    if why.is_none() && marker::write_no_reuse(data, agent).is_err() {
        return job.failed(SigninCode::Failed);
    }
    // The lock rule: the link and `login-source` change only under the lock on Codex's login
    // files (`SigninHub::codex_files`). Held from here through `clear_source`, so a "Use it"
    // this job replaced has either linked and recorded (and both go here) or, at its last
    // check, sees it was replaced and leaves nothing; the two never interleave. No caller holds
    // it: Codex's `sign_out` lets it go before its status check.
    let _files = match job.vendor {
        Vendor::Codex => Some(job.shared.signin.codex_files().await),
        Vendor::Claude => None,
    };
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
    SigninState::SignedOut {
        found: Some(why.unwrap_or(Found::ReuseOff)),
    }
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
        let found = detect::claude(job, cancel)?;
        // Detection gives no email (a stat, `detect`'s docs), so a recorded one (from before)
        // is compared only against an email detection did give; #554 enforces the account.
        return match &found.state {
            SigninState::LoginFound { email: now, .. } if now.is_none() || *now == email => {
                claude_session(job, email, found, false).await
            }
            SigninState::LoginFound { .. } => Some(stop(job, Some(Found::ReusedChanged)).await),
            _ => Some(stop(job, Some(Found::ReusedSignedOut)).await),
        };
    }
    if let Some(identity) = claude::status(job, cancel).await? {
        return Some(claude::signed_in(job, identity));
    }
    if marker::no_reuse(&job.config.data_dir, &job.ticket.agent) {
        return Some(SigninState::SignedOut {
            found: Some(Found::ReuseOff),
        });
    }
    Some(detect::claude(job, cancel)?.state)
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
        return Some(stop(job, None).await);
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
    Some(reused_signed_in(job, session.email.or(email), plan))
}

async fn check_codex<T: Timer + Send + Sync>(job: &SigninJob<T>) -> Option<SigninState> {
    let cancel = &job.ticket.cancel;
    let source = marker::read_source(&job.config.data_dir, &job.ticket.agent);
    if reuse::codex_reused(source.as_ref(), &job.home) {
        let Some(lawyer) = job.config.signin.lawyer_home() else {
            return Some(stop(job, Some(Found::CodexLinkSplit)).await);
        };
        if let Some(why) = reuse::codex_link_holds(&job.home, &lawyer).found() {
            return Some(stop(job, Some(why)).await);
        }
        // The marker and the link say only that the login was chosen; Codex's own status
        // through its CLI must still confirm it.
        let reused = RunKind::Reused { home: &job.home };
        return match confirm_twice(job, reused).await {
            Confirmation::Confirmed(identity) => {
                let accepted = match source {
                    Some(LoginSource::Reused { email }) => email,
                    _ => None,
                };
                Some(reused_signed_in(
                    job,
                    identity.email.or(accepted),
                    identity.plan,
                ))
            }
            Confirmation::SignedOut => Some(stop(job, Some(Found::ReusedSignedOut)).await),
            Confirmation::Unknown => None,
        };
    }
    match confirm_twice(job, RunKind::Own { home: &job.home }).await {
        Confirmation::Confirmed(identity) => return Some(codex::signed_in(job, identity)),
        Confirmation::SignedOut => {}
        Confirmation::Unknown => return None,
    }
    if marker::no_reuse(&job.config.data_dir, &job.ticket.agent) {
        return Some(SigninState::SignedOut {
            found: Some(Found::ReuseOff),
        });
    }
    Some(detect::codex(job, cancel)?.state)
}

/// The limit of the status check's first run at start: 15 s. A post-reboot run takes about 11 s
/// on Windows, so it is not killed as a 5 s one was (the Claude limit, [`claude::STATUS_LIMIT`],
/// and Codex's 15 s are both no longer than this).
pub(crate) const FIRST_LIMIT: Duration = Duration::from_secs(15);
/// The limit of the one retry after the first run got no answer (a run killed at its limit): 20 s.
pub(crate) const RETRY_LIMIT: Duration = Duration::from_secs(20);
/// The waits before the first background re-checks once Sign in is shown after the retries:
/// 30 s, then 60 s; [`RECHECK_CAP`] each time after.
pub(crate) const RECHECK_BACKOFF: [Duration; 2] =
    [Duration::from_secs(30), Duration::from_secs(60)];
/// The longest wait between background re-checks.
pub(crate) const RECHECK_CAP: Duration = Duration::from_secs(120);
/// The limit of the quick status check a sign-in runs first.
pub(crate) const QUICK_LIMIT: Duration = Duration::from_secs(5);

/// The status check job (`check_status`, at start and when desktop asks again), so the card is
/// never left with nothing on it:
/// - with no state shown yet, `Checking` first;
/// - the first run has a 15 s limit ([`FIRST_LIMIT`]); a check that could not tell (a run killed
///   at its limit) is asked once more with a 20 s one ([`RETRY_LIMIT`]);
/// - still no answer: a state already shown stays (Codex's kept rule); with none shown, a
///   `warn` naming the limits, `SignedOut` (Sign in), and re-checks in the background after
///   30 s, 60 s, then every 120 s ([`RECHECK_BACKOFF`]) until one answers, which is the state.
///   That late answer always flips the card.
///   A sign-in, sign-out or any other job supersedes the job and ends them, as does shutdown.
pub(crate) async fn start_check<T: Timer + Send + Sync>(job: &SigninJob<T>) -> Option<SigninState> {
    let hub = &job.shared.signin;
    let cancel = &job.ticket.cancel;
    let blank = matches!(
        hub.state(&job.ticket.agent),
        None | Some(SigninState::Checking)
    );
    if blank {
        job.report(SigninState::Checking);
    }
    let mut tried = Vec::with_capacity(2);
    for limit in [FIRST_LIMIT, RETRY_LIMIT] {
        if !hub.is_live(&job.ticket) {
            return None;
        }
        job.set_status_limit(Some(limit));
        tried.push(limit);
        if let Some(state) = check(job).await {
            return Some(state);
        }
    }
    if !blank || !hub.is_live(&job.ticket) {
        return None;
    }
    let tried: Vec<String> = tried
        .iter()
        .map(|limit| format!("{} s", limit.as_secs()))
        .collect();
    tracing::warn!(
        agent = %job.ticket.agent,
        timeouts = %tried.join(", "),
        "sign-in status check got no answer within each limit: showing Sign in, checking again in the background"
    );
    job.report(SigninState::SignedOut { found: None });
    let mut waits = RECHECK_BACKOFF
        .into_iter()
        .chain(std::iter::repeat(RECHECK_CAP));
    loop {
        let wait = waits.next()?;
        if let Either::Right(_) =
            select(pin!(job.timer.after(wait)), pin!(cancel.cancelled())).await
        {
            return None;
        }
        if !hub.is_live(&job.ticket) {
            return None;
        }
        if let Some(state) = check(job).await {
            return Some(state);
        }
    }
}

/// The quick status check a sign-in makes first, on Clauseo's own login with a 5 s limit
/// ([`QUICK_LIMIT`]): `SignedIn` when it says signed in, so no login runs and no browser opens.
/// Anything else, a timeout included, is `None`: the sign-in goes on as usual. Never for a
/// Codex home whose login file is the lawyer's (reused, or one file with theirs): its status
/// would read the lawyer's login as Clauseo's own, so the sign-in's own guard decides.
pub(crate) async fn quick_check<T: Timer + Send + Sync>(job: &SigninJob<T>) -> Option<SigninState> {
    let cancel = &job.ticket.cancel;
    if job.vendor == Vendor::Codex
        && (is_reused(job)
            || job
                .config
                .signin
                .lawyer_home()
                .is_some_and(|lawyer| reuse::codex_login_shared(&job.home, &lawyer)))
    {
        return None;
    }
    // The card shows `Checking` while the quick check runs, as at start (`start_check`), so a
    // press that waits up to the limit never looks unchanged. The sign-in flow publishes its own
    // states once the check has answered "not signed in" or timed out.
    job.report(SigninState::Checking);
    job.set_status_limit(Some(QUICK_LIMIT));
    let own = RunKind::Own { home: &job.home };
    let state = match job.vendor {
        Vendor::Claude => claude::status(job, cancel)
            .await
            .flatten()
            .map(|identity| claude::signed_in(job, identity)),
        Vendor::Codex => match codex::status_run(job, cancel, own, &job.home).await {
            codex::StatusSaid::LoggedIn => {
                match codex::confirm_account(job, cancel, own, &job.home).await {
                    Confirmation::Confirmed(identity) => Some(codex::signed_in(job, identity)),
                    Confirmation::SignedOut | Confirmation::Unknown => None,
                }
            }
            _ => None,
        },
    };
    job.set_status_limit(None);
    if cancel.is_cancelled() {
        return None;
    }
    if state.is_some() {
        job.note(SigninNote::StatusConfirmed);
    }
    state
}

/// Codex's own confirmation under `kind` in the home, for the status check: asked once more when
/// the first got no answer at all (a run killed at its limit, or one that did not start),
/// unless the job was cancelled. Still `Unknown` after that, the
/// `StatusNotConfirmed { Kept }` note says the state is left as it was. A slow cold start (the
/// first run warms the CLI) or one stalled run is then no reason to end a reuse or restart the
/// agents.
async fn confirm_twice<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    kind: RunKind<'_>,
) -> Confirmation {
    let cancel = &job.ticket.cancel;
    let first = codex::confirm(job, cancel, kind, &job.home).await;
    if first != Confirmation::Unknown || cancel.is_cancelled() {
        return first;
    }
    let second = codex::confirm(job, cancel, kind, &job.home).await;
    if second == Confirmation::Unknown {
        job.note(SigninNote::StatusNotConfirmed {
            reason: Unconfirmed::Kept,
        });
    }
    second
}

/// `find_login` (UD2's "Look for my … login"): "deletes `no-reuse` and detects again", through
/// the status check. It always ends in a state, so the lawyer who asked gets an answer: a check
/// that could not tell is `signin.failed` here, never the state left as it was.
pub(crate) async fn find_login<T: Timer + Send + Sync>(job: &SigninJob<T>) -> Option<SigninState> {
    if marker::remove_no_reuse(&job.config.data_dir, &job.ticket.agent).is_err() {
        return Some(job.failed(SigninCode::Failed));
    }
    Some(
        check(job)
            .await
            .unwrap_or_else(|| job.failed(SigninCode::Failed)),
    )
}

/// `use_found_login` (the lawyer's "Use it"): "R20 writes Claude's env, or S18 makes Codex's
/// link; then `login-source` `reused`; then `SignedIn { source: Reused }`." Detection runs
/// again first, and "Use it" goes on while it still finds a login and its email equals the one
/// shown. **That does not tie "Use it" to an account:** detection is a stat of each agent's
/// login file and gives no email, so `None` is compared with `None`, and a different account
/// signed in between "found" and "Use it" is reused. What enforces the account is R20's
/// session-start identity check (#554), which compares the session's account with the one the
/// lawyer accepted. A login gone in between is not reused:
/// the new detection is the state. Detection that could not tell ends the job:
/// `signin.cancelled` when it was cancelled (or the job replaced), else `signin.failed`.
pub(crate) async fn use_found<T: Timer + Send + Sync>(job: &SigninJob<T>) -> SigninState {
    let cancel = &job.ticket.cancel;
    let shown = match job.shared.signin.state(&job.ticket.agent) {
        Some(SigninState::LoginFound { email, .. }) => email,
        _ => None,
    };
    let found = match job.vendor {
        Vendor::Claude => detect::claude(job, cancel),
        Vendor::Codex => detect::codex(job, cancel),
    };
    if !job.shared.signin.is_live(&job.ticket) {
        return job.failed(SigninCode::Cancelled);
    }
    let Some(found) = found else {
        return job.failed(SigninCode::Failed);
    };
    let email = match &found.state {
        SigninState::LoginFound { email, .. } if *email == shown => email.clone(),
        other => return other.clone(),
    };
    match job.vendor {
        Vendor::Claude => claude_session(job, email, found, true)
            .await
            .unwrap_or_else(|| {
                if cancel.is_cancelled() {
                    job.failed(SigninCode::Cancelled)
                } else {
                    job.failed(SigninCode::Failed)
                }
            }),
        Vendor::Codex => use_codex(job, email).await,
    }
}

/// How Codex's link step ended.
enum Linked {
    /// The link and the `reused` record are made, and Codex's own status confirmed the account.
    Done(Identity),
    /// Codex's own status through the link said signed out (an API-key login, say): the link
    /// was removed again and nothing recorded.
    SignedOut,
    /// Codex's own status through the link gave no answer: the link was removed again.
    NoAnswer,
    /// The job was cancelled or replaced: nothing is left linked or recorded.
    Stopped,
    /// The link could not be made: the fallback runs.
    LinkFailed,
    /// The record could not be written: the link was removed again.
    RecordFailed,
}

/// Codex's "Use it": the link (`reuse::link_reused_login`), Codex's own confirmation through
/// it, then the record. Detection only stats the lawyer's file, so this confirmation is where
/// the email and plan come from, and where a login that is no `ChatGPT` one (an API key) is
/// turned down: `SignedOut { found: ReusedSignedOut }`. "Another volume, or any link error:
/// Clauseo's own sign-in, logged" (the `codex_link_failed` note, then the browser sign-in).
async fn use_codex<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    email: Option<String>,
) -> SigninState {
    match link_and_record(job, email.clone()).await {
        Linked::Done(identity) => reused_signed_in(job, identity.email.or(email), identity.plan),
        Linked::SignedOut => SigninState::SignedOut {
            found: Some(Found::ReusedSignedOut),
        },
        Linked::Stopped => job.failed(SigninCode::Cancelled),
        Linked::NoAnswer | Linked::RecordFailed => job.failed(SigninCode::Failed),
        Linked::LinkFailed => {
            // The lock is let go by now: the browser sign-in takes it for its own run.
            job.note(SigninNote::CodexLinkFailed);
            codex::sign_in(job, Method::Browser).await
        }
    }
}

/// The link, the confirmation and the record, under the lock on Codex's login files. The job
/// must still be the agent's and not cancelled right before the link and again right before the
/// record. Cancelled or replaced before the link: nothing was done, nothing is undone. After it
/// (during the confirmation included): Clauseo's own name goes again (`remove_file` of the
/// home's name, never the lawyer's), and no `reused` is written. The confirmation is Codex's
/// own status in the home, as a reused session sees it (`RunKind::Reused`): a definite "signed
/// out", or no answer, removes the link too. A newer job's Codex login or
/// `use_different_account` waits for this lock, so it never runs while this step could still
/// leave the files linked.
async fn link_and_record<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    email: Option<String>,
) -> Linked {
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
    let reused = RunKind::Reused { home: &job.home };
    let confirmed = codex::confirm(job, &job.ticket.cancel, reused, &job.home).await;
    if !hub.is_live(&job.ticket) {
        drop(reuse::unlink_reused_login(&job.home));
        return Linked::Stopped;
    }
    let identity = match confirmed {
        Confirmation::Confirmed(identity) => identity,
        Confirmation::SignedOut => {
            drop(reuse::unlink_reused_login(&job.home));
            return Linked::SignedOut;
        }
        Confirmation::Unknown => {
            drop(reuse::unlink_reused_login(&job.home));
            return Linked::NoAnswer;
        }
    };
    // The email Codex confirmed, else the one shown (detection's stat gives none).
    let source = LoginSource::Reused {
        email: identity.email.clone().or(email),
    };
    if marker::write_source(&job.config.data_dir, &job.ticket.agent, &source).is_err() {
        // The link alone would read as reused; it goes, and nothing is reused.
        drop(reuse::unlink_reused_login(&job.home));
        return Linked::RecordFailed;
    }
    Linked::Done(identity)
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
pub(crate) async fn stop_using<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    why: Option<Found>,
) -> Option<SigninState> {
    let source = marker::read_source(&job.config.data_dir, &job.ticket.agent);
    if source == Some(LoginSource::Own) {
        return None;
    }
    Some(stop(job, why).await)
}
