//! How a Codex login that did not end signed in ends: the status check after a cancelled,
//! failed or killed login ([`checked_again`]), the confirmation after one that exited 0 or that a
//! poll saw signed in ([`confirmed_or_failed`]), and the removal of a login that never confirmed
//! ([`remove_own_login`]).

use std::ffi::OsString;
use std::pin::pin;

use clauseo_process::Timer;
use futures::future::{Either, select};

use super::{Confirmation, LOGOUT_STOP, confirm, confirm_account, signed_in};
use crate::cancel::{Cancel, Stop};
use crate::signin::claude::LOGOUT_LIMIT;
use crate::signin::env::{RunKind, codex_store_args};
use crate::signin::job::SigninJob;
use crate::signin::run::run;
use crate::signin::{SigninCode, SigninState};

/// How long a follow-up check's run may take to end after a second stop: the lawyer's grace,
/// which also ends inside the quit's bound.
const FOLLOW_UP_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Whether the job is ending for the app's quit, or was stopped at once (replaced, or killed at
/// a bound): no new run starts for it then, since none could finish.
fn ending_now(cancel: &Cancel) -> bool {
    cancel.is_killed() || cancel.quit_seen()
}

/// The end of a login that ran and was cancelled, failed or killed (`failed`, its `Failed`):
/// Codex's `login` deletes the login it replaces before it starts, so the state shown is what a
/// status check says now, never the `SignedIn` from before. Signed in (the login saved before it
/// ended): that login, confirmed. Anything else: `failed`, the sign-in card's signed-out view,
/// and the home keeps no unconfirmed login ([`remove_own_login`]). The check has a cancel of its
/// own, ended by a stop at once (a newer job, the quit's bound); during the quit it does not run.
pub(super) async fn checked_again<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    failed: SigninState,
) -> SigninState {
    let cancel = &job.ticket.cancel;
    if cancel.quit_seen() {
        drop_unmarked_login(job);
        return failed;
    }
    if ending_now(cancel) {
        return failed;
    }
    let own = Cancel::new();
    let check = pin!(async {
        let kind = RunKind::Own { home: &job.home };
        match confirm(job, &own, kind, &job.home).await {
            Confirmation::Confirmed(identity) => Some(identity),
            Confirmation::SignedOut | Confirmation::Unknown => {
                unconfirmed_goes(job, &own).await;
                None
            }
        }
    });
    // A second Cancel, or the quit, stops the check as the lawyer's Cancel does (what runs is
    // awaited, killed past the grace, and nothing new starts); the stop at once kills it.
    let forward = pin!(async {
        cancel.stopped_again().await;
        // The quit lets an answer on its way land (a confirmation is recorded); a second Cancel
        // does not wait for one.
        own.stop(if cancel.quit_seen() {
            Stop::Quit
        } else {
            Stop::Lawyer(FOLLOW_UP_GRACE)
        });
        cancel.killed().await;
        own.cancel();
        futures::future::pending::<()>().await;
    });
    match select(check, forward).await {
        Either::Left((Some(identity), _)) => signed_in(job, identity),
        Either::Left((None, _)) | Either::Right(_) => failed,
    }
}

/// An unconfirmed login in the home goes: with Codex's `logout` ([`remove_own_login`], under
/// `cancel`), except once the quit is seen, when no logout could finish inside its bound (and,
/// cut at the app's exit, would leave the file): then the file is removed directly
/// ([`drop_unmarked_login`]).
async fn unconfirmed_goes<T: Timer + Send + Sync>(job: &SigninJob<T>, cancel: &Cancel) {
    if job.ticket.cancel.quit_seen() {
        drop_unmarked_login(job);
    } else {
        let _gone = remove_own_login(job, cancel).await;
    }
}

/// During the quit nothing runs after the login, whose child is gone by now. A login file it
/// saved (its `/cancel` waits behind a save already going) has no `login-source` record, and
/// at the next start an unmarked file in the home reads as reused, whose split check would sign
/// the login out. So such a file goes now: the guard (`reuse::codex_login_shared`), then the
/// file removed directly (`reuse::remove_home_login`, which never reads it). A file
/// under an `own` record is left: it reads as Clauseo's own, and its status says how it stands.
fn drop_unmarked_login<T>(job: &SigninJob<T>) {
    let source = crate::signin::marker::read_source(&job.config.data_dir, &job.ticket.agent);
    if source == Some(crate::signin::marker::LoginSource::Own) {
        return;
    }
    let Some(lawyer) = job.config.signin.lawyer_home() else {
        return;
    };
    if crate::signin::reuse::codex_login_shared(&job.home, &lawyer) {
        return;
    }
    drop(crate::signin::reuse::remove_home_login(&job.home));
}

/// The end of a login the lawyer started: [`confirm`]'s `SignedIn`, else `signin.cancelled`
/// when it was cancelled meanwhile, else `signin.failed`. Only such a sign-in may fail; a
/// status check that confirms nothing is never `Failed`. `polled`: a poll's `login status`
/// just said signed in, so only `account/read` is left to confirm.
pub(super) async fn confirmed_or_failed<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
    polled: bool,
) -> SigninState {
    // The quit's bound leaves no time for a check or a logout: an unconfirmed login goes.
    if cancel.quit_seen() {
        drop_unmarked_login(job);
        return job.failed(SigninCode::Cancelled);
    }
    // The lawyer's Cancel came while the login saved: what Codex says now is shown.
    if cancel.is_cancelled() && !cancel.is_killed() {
        return checked_again(job, job.failed(SigninCode::Cancelled)).await;
    }
    let own = RunKind::Own { home: &job.home };
    let confirmation = if polled {
        confirm_account(job, cancel, own, &job.home).await
    } else {
        confirm(job, cancel, own, &job.home).await
    };
    if let Confirmation::Confirmed(identity) = confirmation {
        return signed_in(job, identity);
    }
    // The login ran, so the home may hold its file: unconfirmed, it goes. A removal that failed
    // leaves it; nothing more can be done here.
    unconfirmed_goes(job, &Cancel::new()).await;
    if cancel.is_cancelled() {
        job.failed(SigninCode::Cancelled)
    } else {
        job.failed(SigninCode::Failed)
    }
}

/// Clauseo's own login in the home goes, for a login that ran and did not confirm, and for a
/// sign-out with no CLI: Codex's `logout` through the CLI when it is there (with `cancel`, the
/// caller's own, so a cancelled sign-in still cleans up; once it is set no logout starts), then
/// the home's `auth.json` removed directly when it is still there (`reuse::remove_home_login`,
/// which never reads it). It is Clauseo's own home, and the caller holds the lock on Codex's
/// login files. Never while the home's file is the lawyer's (the guard,
/// [`crate::signin::reuse::codex_login_shared`]), and never when the
/// lawyer's home folder is not known, since the guard cannot then tell: nothing is touched.
/// `true` when the home holds no login file afterwards.
pub(super) async fn remove_own_login<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
) -> bool {
    let Some(lawyer) = job.config.signin.lawyer_home() else {
        return false;
    };
    if crate::signin::reuse::codex_login_shared(&job.home, &lawyer) {
        return false;
    }
    if job.script().is_some() {
        let kind = RunKind::Own { home: &job.home };
        let mut args = codex_store_args(kind);
        args.push(OsString::from("logout"));
        let mut spec = job.spec(args, job.env(kind, None), LOGOUT_LIMIT);
        spec.on_stop = LOGOUT_STOP;
        // Its exit does not matter: the file is checked and removed below either way.
        drop(
            run(
                &*job.launcher,
                &*job.timer,
                spec,
                cancel,
                None,
                |_: &str| {},
            )
            .await,
        );
    }
    crate::signin::reuse::remove_home_login(&job.home).is_ok()
}
