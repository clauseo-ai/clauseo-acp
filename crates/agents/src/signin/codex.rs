//! Codex's flow: a hidden `node <codex.js> -c cli_auth_credentials_store="file" login` (or
//! `login --device-auth`) in Clauseo's own `CODEX_HOME`, confirmed by `login status` and then
//! `app-server`'s `account/read`; signed out by `logout` (the brief, "Codex's commands").
//! `codex.js` is the Codex CLI in the install (`Installed::codex_cli`), never the install's
//! `script`, which is codex-acp's ACP adapter: it ignores these commands and exits 0.
//!
//! **Confirmed** ([`confirm`]) means both: `login status` exited 0 with its `Logged in using …`
//! line, and `account/read` then named an account. Exit 0 alone, a run that timed out, a
//! JSON-RPC error and `account: null` all confirm nothing, and nothing unconfirmed is ever
//! `SignedIn` (or writes `login-source`).
//!
//! Every run here has the Own environment: `CODEX_HOME` and, on macOS and Linux, `HOME` set to
//! the agent's home, `CODEX_API_KEY=""`, and `SSL_CERT_FILE` for the agent's CA.
//!
//! **Browser** (`Method::Browser`): before the run, `127.0.0.1:1455` (the callback port) is bound
//! and let go at once. Taken: no run, `Failed { code: PortBusy }`. "Try again" ([`try_again`])
//! probes it every 2 s and starts the run once it is free. Codex opens the browser from its own
//! process, so this flow never sends `OpenUrl`, and the addresses Codex prints are ignored.
//!
//! **Device code** (`Method::DeviceCode`): the URL and code are read from the run's output
//! ([`device`]) and sent once as `DeviceCode { url, code, expires_at }`, `expires_at` 15 minutes
//! after the clock's now when the code was read. At 15 minutes the run is killed:
//! `signin.link_expired`. Codex's refusal when device code is off: `signin.device_code_off`.
//!
//! While a login runs the state is `Waiting { since, nudge: false }` (no nudge: the screen offers
//! "Use a code instead" from the start), and `login status` is polled every 1.5 s with a 5 s
//! limit, as for Claude. The first poll that says signed in kills the login; then [`confirm`]
//! gives `SignedIn { email, plan, source: Own, at }`, or `signin.failed` when it does not
//! confirm. It ends otherwise:
//! - on Cancel or a newer job, killed: `signin.cancelled`;
//! - at the run's limit (10 minutes, 20 for device code, whose code expires first), killed:
//!   `signin.link_expired`;
//! - by exiting 0: [`confirm`] decides between `SignedIn` and `signin.failed`;
//! - by exiting otherwise: `signin.device_code_off`, `signin.port_busy` or `signin.cancelled`
//!   when a line said so, else `signin.failed`.
//!
//! The login, its pollers and the app-server are all gone before the last state is sent.

mod device;
pub(crate) mod identity;

use std::ffi::OsString;
use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;
use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use clauseo_process::Timer;
use futures::channel::oneshot;
use futures::future::{Either, select};

use self::device::{CODE_LIFETIME, DeviceLines, is_device_code_off, is_port_busy};
use crate::cancel::Cancel;
use crate::signin::claude::{LOGIN_LIMIT, LOGOUT_LIMIT, POLL, STATUS_LIMIT, is_cancel_line};
use crate::signin::env::{RunKind, codex_store_args};
use crate::signin::job::SigninJob;
use crate::signin::run::{Ended, Ran, run};
use crate::signin::{Method, SigninCode, SigninNote, SigninState, Source, Unconfirmed};
use crate::status::{Identity, codex_logged_in, codex_says_logged_in};

/// How often "Try again" probes the callback port.
pub(crate) const PORT_POLL: Duration = Duration::from_secs(2);
/// The device-code run's own limit: past the code's 15 minutes, so the code's expiry is what
/// ends it, and a run that never prints a code still ends.
pub(crate) const DEVICE_LIMIT: Duration = Duration::from_mins(20);

/// Whether `port` on `127.0.0.1` is free now: it can be bound. The listener is dropped at once,
/// before Codex binds it.
pub(crate) fn port_free(port: u16) -> bool {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok()
}

/// The brief's guard: before any Codex `login` or `logout`, the home's login file and the
/// lawyer's `~/.codex` one are compared by file id ([`super::reuse::codex_login_shared`]), and
/// the run is refused while they are one file, whatever `login-source` says. With no lawyer's
/// home folder known there is no lawyer's file to share.
fn home_shares_lawyers_login<T>(job: &SigninJob<T>) -> bool {
    job.config
        .signin
        .lawyer_home()
        .is_some_and(|lawyer| super::reuse::codex_login_shared(&job.home, &lawyer))
}

/// A sign-in with `method`, start to end; gives the last state.
pub(crate) async fn sign_in<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    method: Method,
) -> SigninState {
    match method {
        Method::Browser => {
            if !port_free(job.config.signin.callback_port()) {
                return job.failed(SigninCode::PortBusy);
            }
            login(job, false).await
        }
        Method::DeviceCode => login(job, true).await,
        // The call refuses Claude's methods for Codex.
        _ => job.failed(SigninCode::Failed),
    }
}

/// "Try again" after `signin.port_busy`: the port is probed now and every 2 s, with the state
/// left as it is, and the browser sign-in starts once it is free. Cancel stops the wait.
pub(crate) async fn try_again<T: Timer + Send + Sync>(job: &SigninJob<T>) -> SigninState {
    let cancel = &job.ticket.cancel;
    while !port_free(job.config.signin.callback_port()) {
        let wait = pin!(job.timer.after(PORT_POLL));
        let cancelled = pin!(cancel.cancelled());
        if let Either::Right(_) = select(wait, cancelled).await {
            return job.failed(SigninCode::Cancelled);
        }
    }
    login(job, false).await
}

/// How the login's watch ended first.
enum First {
    /// The login run ended (or could not start).
    Ran(Result<Ran, clauseo_process::Error>),
    /// A poll said signed in.
    Confirmed,
    /// The device code's 15 minutes passed.
    Expired,
}

/// What the login's output said, line by line.
#[derive(Default)]
struct Said {
    denied: AtomicBool,
    device_off: AtomicBool,
    port_busy: AtomicBool,
}

/// The login run (`device`: `--device-auth`), watched to its end. The lock on Codex's login
/// files is taken first and held to the end, so the guard looks only once any "Use it" this job
/// replaced has finished or undone its link, and no "Use it" links while this login runs.
async fn login<T: Timer + Send + Sync>(job: &SigninJob<T>, device: bool) -> SigninState {
    let _files = job.shared.signin.codex_files().await;
    if !job.shared.signin.is_live(&job.ticket) {
        return job.failed(SigninCode::Cancelled);
    }
    if crate::write::create_private_dirs_under(&job.config.data_dir, &job.home).is_err()
        || home_shares_lawyers_login(job)
    {
        return job.failed(SigninCode::Failed);
    }
    let kind = RunKind::Own { home: &job.home };
    let mut args = codex_store_args(kind);
    args.push(OsString::from("login"));
    if device {
        args.push(OsString::from("--device-auth"));
    }
    let limit = if device { DEVICE_LIMIT } else { LOGIN_LIMIT };
    let spec = job.spec(args, job.env(kind, None), limit);
    job.report(SigninState::Waiting {
        since: job.now(),
        nudge: false,
    });
    let said = Said::default();
    let lines = Mutex::new(DeviceLines::default());
    let (code_read, code_seen) = oneshot::channel::<()>();
    let code_read = Mutex::new(Some(code_read));
    let cancel = &job.ticket.cancel;
    let first = {
        let login = pin!(run(
            &*job.launcher,
            &*job.timer,
            spec,
            cancel,
            None,
            |line: &str| {
                note(&said, line);
                if !device {
                    return;
                }
                let found = lines
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .feed(line, job.hosts());
                if let Some((url, code)) = found {
                    job.report(SigninState::DeviceCode {
                        url,
                        code,
                        expires_at: job.now() + CODE_LIFETIME,
                    });
                    if let Some(read) = code_read
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take()
                    {
                        // The watch is still there while the run is.
                        read.send(()).unwrap_or_default();
                    }
                }
            },
        ));
        let watch = pin!(async {
            loop {
                job.timer.after(POLL).await;
                if status(job, cancel).await == Some(true) {
                    return;
                }
            }
        });
        // The code's 15 minutes run from when it was read.
        let expiry = pin!(async {
            if code_seen.await.is_ok() {
                job.timer.after(CODE_LIFETIME).await;
            } else {
                futures::future::pending::<()>().await;
            }
        });
        match select(login, select(watch, expiry)).await {
            Either::Left((ran, _)) => First::Ran(ran),
            Either::Right((Either::Left(((), _)), _)) => First::Confirmed,
            Either::Right((Either::Right(((), _)), _)) => First::Expired,
        }
    };
    match first {
        First::Confirmed => confirmed_or_failed(job, cancel, true).await,
        First::Expired => job.failed(SigninCode::LinkExpired),
        First::Ran(Err(_)) => {
            job.note(SigninNote::LoginExited { code: None });
            job.failed(SigninCode::Failed)
        }
        First::Ran(Ok(ran)) => {
            if let Ended::Exited(code) = ran.ended {
                job.note(SigninNote::LoginExited { code });
            }
            match ran.ended {
                Ended::Cancelled => job.failed(SigninCode::Cancelled),
                Ended::TimedOut => job.failed(SigninCode::LinkExpired),
                Ended::Exited(Some(0)) => confirmed_or_failed(job, cancel, false).await,
                Ended::Exited(_) => job.failed(refusal(&said)),
            }
        }
    }
}

/// The end of a login the lawyer started: [`confirm`]'s `SignedIn`, else `signin.cancelled`
/// when it was cancelled meanwhile, else `signin.failed`. Only such a sign-in may fail; a
/// status check that confirms nothing is never `Failed`. `polled`: a poll's `login status`
/// just said signed in, so only `account/read` is left to confirm.
async fn confirmed_or_failed<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
    polled: bool,
) -> SigninState {
    let own = RunKind::Own { home: &job.home };
    let confirmation = if polled {
        confirm_account(job, cancel, own, &job.home).await
    } else {
        confirm(job, cancel, own, &job.home).await
    };
    match confirmation {
        Confirmation::Confirmed(identity) => signed_in(job, identity),
        _ if cancel.is_cancelled() => job.failed(SigninCode::Cancelled),
        Confirmation::SignedOut | Confirmation::Unknown => job.failed(SigninCode::Failed),
    }
}

/// What [`confirm`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Confirmation {
    /// `login status` said signed in and `account/read` named this account.
    Confirmed(Identity),
    /// `login status` said signed out: it exited non-zero (`Not logged in`, exit 1).
    SignedOut,
    /// Nothing either way: no answer, exit 0 with no signed-in line, or no account read.
    Unknown,
}

/// Codex's own confirmation of a login under `kind`'s environment in `cwd`: `login status`
/// exits 0 with its signed-in line, then `account/read` names an account. Logs the outcome as
/// a note, by a fixed word and never the account.
pub(crate) async fn confirm<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
    kind: RunKind<'_>,
    cwd: &Path,
) -> Confirmation {
    let not = |reason| {
        job.note(SigninNote::StatusNotConfirmed { reason });
    };
    match status_run(job, cancel, kind, cwd).await {
        StatusSaid::LoggedIn => confirm_account(job, cancel, kind, cwd).await,
        StatusSaid::SignedOut => {
            not(Unconfirmed::SignedOut);
            Confirmation::SignedOut
        }
        StatusSaid::NoMarker => {
            not(Unconfirmed::NoMarker);
            Confirmation::Unknown
        }
        StatusSaid::NoAnswer => {
            not(Unconfirmed::NoAnswer);
            Confirmation::Unknown
        }
    }
}

/// [`confirm`]'s second half, once `login status` said signed in: `account/read` must name
/// an account.
async fn confirm_account<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
    kind: RunKind<'_>,
    cwd: &Path,
) -> Confirmation {
    if let Some(identity) = identity::read_as(job, cancel, kind, cwd).await {
        job.note(SigninNote::StatusConfirmed);
        Confirmation::Confirmed(identity)
    } else {
        job.note(SigninNote::StatusNotConfirmed {
            reason: Unconfirmed::NoAccount,
        });
        Confirmation::Unknown
    }
}

/// Records what one output line says.
fn note(said: &Said, line: &str) {
    if is_device_code_off(line) {
        said.device_off.store(true, Ordering::SeqCst);
    }
    if is_port_busy(line) {
        said.port_busy.store(true, Ordering::SeqCst);
    }
    if is_cancel_line(line) {
        said.denied.store(true, Ordering::SeqCst);
    }
}

/// The code of a login that exited non-zero, from what its lines said.
fn refusal(said: &Said) -> SigninCode {
    if said.device_off.load(Ordering::SeqCst) {
        SigninCode::DeviceCodeOff
    } else if said.port_busy.load(Ordering::SeqCst) {
        SigninCode::PortBusy
    } else if said.denied.load(Ordering::SeqCst) {
        SigninCode::Cancelled
    } else {
        SigninCode::Failed
    }
}

/// `SignedIn` for `identity`, Clauseo's own login, now.
pub(crate) fn signed_in<T>(job: &SigninJob<T>, identity: Identity) -> SigninState {
    SigninState::SignedIn {
        email: identity.email,
        plan: identity.plan,
        source: Source::Own,
        at: job.config.clock.now(),
    }
}

/// One `login status` in Clauseo's home, for the login's polls: `Some(true)` signed in (exit 0
/// with the signed-in line), `Some(false)` not, `None` when it could not tell (no start,
/// killed past 5 s, cancelled).
pub(crate) async fn status<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
) -> Option<bool> {
    match status_run(job, cancel, RunKind::Own { home: &job.home }, &job.home).await {
        StatusSaid::LoggedIn => Some(true),
        StatusSaid::SignedOut | StatusSaid::NoMarker => Some(false),
        StatusSaid::NoAnswer => None,
    }
}

/// What one `login status` said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusSaid {
    /// Exit 0 with the `Logged in using …` line.
    LoggedIn,
    /// A non-zero exit (`Not logged in`, exit 1).
    SignedOut,
    /// Exit 0 with no signed-in line: not Codex's CLI answering (codex-acp's adapter exits 0 for
    /// any arguments), so not a login.
    NoMarker,
    /// No answer: it did not start, was killed past its limit, or was cancelled.
    NoAnswer,
}

/// One `login status` with `kind`'s environment and store argument, in `cwd`. Its lines are
/// only checked for the signed-in start ([`codex_says_logged_in`]) and never kept: stderr
/// prints a masked key prefix.
pub(crate) async fn status_run<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
    kind: RunKind<'_>,
    cwd: &Path,
) -> StatusSaid {
    let mut args = codex_store_args(kind);
    args.extend([OsString::from("login"), OsString::from("status")]);
    let mut spec = job.spec(args, job.env(kind, None), STATUS_LIMIT);
    spec.cwd = cwd.to_path_buf();
    let said_in = AtomicBool::new(false);
    let ran = run(
        &*job.launcher,
        &*job.timer,
        spec,
        cancel,
        None,
        |line: &str| {
            if codex_says_logged_in(line) {
                said_in.store(true, Ordering::SeqCst);
            }
        },
    )
    .await;
    match ran.map(|ran| ran.ended) {
        Ok(Ended::Exited(code)) if codex_logged_in(code, said_in.load(Ordering::SeqCst)) => {
            StatusSaid::LoggedIn
        }
        Ok(Ended::Exited(Some(0))) => StatusSaid::NoMarker,
        Ok(Ended::Exited(_)) => StatusSaid::SignedOut,
        Ok(Ended::TimedOut | Ended::Cancelled) | Err(_) => StatusSaid::NoAnswer,
    }
}

/// `logout` in Clauseo's home, then a status check. Never for a reused login (the call refuses
/// it by the state and the job checks again here by `login-source` and the home's login file),
/// and never while the home shares the lawyer's login file (the guard,
/// [`home_shares_lawyers_login`]): Codex's logout revokes the tokens and deletes the login. The
/// job then runs the status check instead.
pub(crate) async fn sign_out<T: Timer + Send + Sync>(job: &SigninJob<T>) -> Option<SigninState> {
    // Held from the checks to the logout's end, as for a login.
    let files = job.shared.signin.codex_files().await;
    if !job.shared.signin.is_live(&job.ticket) {
        return None;
    }
    if job.shared.signin.source(&job.ticket.agent) == Some(Source::Reused)
        || super::found::is_reused(job)
        || home_shares_lawyers_login(job)
    {
        drop(files);
        return super::found::check(job).await;
    }
    let kind = RunKind::Own { home: &job.home };
    let mut args = codex_store_args(kind);
    args.push(OsString::from("logout"));
    let spec = job.spec(args, job.env(kind, None), LOGOUT_LIMIT);
    let cancel = &job.ticket.cancel;
    let ran = run(
        &*job.launcher,
        &*job.timer,
        spec,
        cancel,
        None,
        |_: &str| {},
    )
    .await;
    drop(files);
    if cancel.is_cancelled() {
        return None;
    }
    match confirm(job, cancel, kind, &job.home).await {
        Confirmation::Confirmed(identity) => Some(signed_in(job, identity)),
        Confirmation::SignedOut => Some(SigninState::SignedOut { found: None }),
        // Nothing confirmed: a logout that exited 0 is taken at its word; else the state is
        // left as it is, unless that is `SignedIn`, which nothing confirms now.
        Confirmation::Unknown => ran
            .ok()
            .filter(|ran| ran.ended == Ended::Exited(Some(0)))
            .map(|_| SigninState::SignedOut { found: None })
            .or_else(|| super::found::unconfirmed(job)),
    }
}

#[cfg(test)]
mod tests;
