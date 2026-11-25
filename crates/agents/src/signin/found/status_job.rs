//! The status check job at start and the quick check a sign-in makes first, with their limits:
//! how long each run may take, the one retry, and the background re-checks once Sign in is
//! shown. What a check answers is `found`'s ([`super::check`]); this module only times and
//! repeats it.

use std::pin::pin;
use std::time::Duration;

use clauseo_process::Timer;
use futures::future::{Either, select};

use super::{check, is_reused};
use crate::signin::codex::Confirmation;
use crate::signin::env::{RunKind, Vendor};
use crate::signin::job::SigninJob;
use crate::signin::{SigninCode, SigninNote, SigninState, claude, codex, reuse};

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

/// A sign-in pressed while the agent's login is the lawyer's, reused (Clauseo's record), or, for
/// Codex, while the home's login file is one file with theirs: no login runs over it (Codex's
/// guard refuses one, and a Claude login would make a second account beside the one in use).
/// The status check runs instead, as at start ([`FIRST_LIMIT`], one retry), and its answer ends
/// the sign-in: `SignedIn` when the login holds, else what the check says (a reuse that no
/// longer holds stops, so the next press signs in). So a press that meets the install's first
/// status check, which it supersedes, still ends signed in. `None` when neither applies, or the
/// check could not tell: the sign-in goes on as before. Cancelled meanwhile: `signin.cancelled`.
pub(crate) async fn reused_at_press<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
) -> Option<SigninState> {
    let shared = job.vendor == Vendor::Codex
        && job
            .config
            .signin
            .lawyer_home()
            .is_some_and(|lawyer| reuse::codex_login_shared(&job.home, &lawyer));
    if !(shared || is_reused(job)) {
        return None;
    }
    tracing::info!(agent = %job.ticket.agent, "sign in: the login is reused; the status check answers");
    job.report(SigninState::Checking);
    let mut state = None;
    for limit in [FIRST_LIMIT, RETRY_LIMIT] {
        if state.is_some() || job.ticket.cancel.is_cancelled() {
            break;
        }
        job.set_status_limit(Some(limit));
        state = check(job).await;
    }
    job.set_status_limit(None);
    if job.ticket.cancel.is_cancelled() {
        return Some(job.failed(SigninCode::Cancelled));
    }
    state
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
