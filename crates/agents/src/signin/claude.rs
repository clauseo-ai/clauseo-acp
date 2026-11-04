//! Claude's flow: a hidden `node <adapter> --cli auth login --claudeai|--console` in Clauseo's
//! own folder (`CLAUDE_CONFIG_DIR` = the agent's home), confirmed by `--cli auth status --json`,
//! signed out by `--cli auth logout` (the brief, "Claude's commands").
//!
//! While the login runs (research 22 §3.1):
//! - the state is `Waiting { since, nudge: false }`, and `nudge` turns true at 30 s ("Didn't
//!   open?"; reopening is a new `sign_in`, which supersedes this run);
//! - status is polled every 1.5 s, each poll killed past 5 s; the first that says signed in ends
//!   the run (its child killed) with `SignedIn { email, plan, source: Own, at }`;
//! - on macOS and Linux, the shim's URL file is read on each output line and each poll, and each
//!   new address on a sign-in host goes out once as `OpenUrl` ([`super::shim`]);
//! - a pasted code goes to the login's stdin.
//!
//! It ends:
//! - at 10 minutes, killed: `signin.link_expired`;
//! - on Cancel, killed: `signin.cancelled`;
//! - by exiting 0: one more status run decides between `SignedIn` and `signin.failed`;
//! - by exiting otherwise: `signin.cancelled` when a line matched the cancel pattern, else
//!   `signin.failed`.
//!
//! In every case the login's child, its pollers and its URL file are gone before the last state
//! is sent, so no status process runs after it.

use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use clauseo_process::Timer;
use futures::future::{Either, select};

use crate::cancel::Cancel;
use crate::signin::env::{RunKind, Shim};
use crate::signin::job::SigninJob;
use crate::signin::run::{Ended, Ran, run};
use crate::signin::shim::UrlFile;
use crate::signin::{PasteCode, SigninCode, SigninState, Source};
use crate::status::{Identity, claude_signed_in, parse_claude_status};

/// The time between status polls during a login.
pub(crate) const POLL: Duration = Duration::from_millis(1500);
/// The most one status run may take.
pub(crate) const STATUS_LIMIT: Duration = Duration::from_secs(5);
/// When "Didn't open?" is offered.
pub(crate) const NUDGE: Duration = Duration::from_secs(30);
/// When a login run is killed and its link counts as expired.
pub(crate) const LOGIN_LIMIT: Duration = Duration::from_mins(10);
/// The most a logout may take.
pub(crate) const LOGOUT_LIMIT: Duration = Duration::from_secs(30);

/// The login's arguments after the script.
const LOGIN_CLAUDE_AI: [&str; 4] = ["--cli", "auth", "login", "--claudeai"];
const LOGIN_CONSOLE: [&str; 4] = ["--cli", "auth", "login", "--console"];
const STATUS: [&str; 4] = ["--cli", "auth", "status", "--json"];
const LOGOUT: [&str; 3] = ["--cli", "auth", "logout"];

/// Whether a login's output line says the lawyer refused on the vendor's page: Claude prints
/// `OAuth error: access_denied` (the OAuth error code for a refusal), in any case.
pub(crate) fn is_cancel_line(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("oauth error") && line.contains("access_denied")
}

/// How the login's watch ended first.
enum First {
    /// The login run ended (or could not start).
    Ran(Result<Ran, clauseo_process::Error>),
    /// A poll said signed in.
    Confirmed(Identity),
}

/// A login run, start to end; gives the last state.
pub(crate) async fn sign_in<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    console: bool,
    paste: Option<async_channel::Receiver<PasteCode>>,
) -> SigninState {
    if crate::write::create_dir_all(&job.home).is_err() {
        return job.failed(SigninCode::Failed);
    }
    let url_file = match url_file(job) {
        Ok(file) => file.map(Mutex::new),
        Err(_) => return job.failed(SigninCode::Failed),
    };
    let url_path = url_file
        .as_ref()
        .map(|file| lock(file).path().to_path_buf());
    let shim = job
        .browser
        .as_deref()
        .zip(url_path.as_deref())
        .map(|(browser, url_file)| Shim { browser, url_file });
    let env = job.env(RunKind::Own { home: &job.home }, shim);
    let args = if console {
        LOGIN_CONSOLE
    } else {
        LOGIN_CLAUDE_AI
    };
    let spec = job.spec(args, env, LOGIN_LIMIT);
    let since = job.now();
    job.report(SigninState::Waiting {
        since,
        nudge: false,
    });
    let open_new = || {
        if let Some(file) = &url_file {
            for url in lock(file).take_new(job.hosts()) {
                job.open_url(url);
            }
        }
    };
    let denied = AtomicBool::new(false);
    let cancel = &job.ticket.cancel;
    let first = {
        let login = pin!(run(
            &*job.launcher,
            &*job.timer,
            spec,
            cancel,
            paste,
            |line: &str| {
                if is_cancel_line(line) {
                    denied.store(true, Ordering::SeqCst);
                }
                open_new();
            },
        ));
        let watch = pin!(async {
            loop {
                job.timer.after(POLL).await;
                open_new();
                if let Some(Some(identity)) = status(job, cancel).await {
                    return identity;
                }
            }
        });
        let nudge = pin!(async {
            job.timer.after(NUDGE).await;
            job.report(SigninState::Waiting { since, nudge: true });
            futures::future::pending::<std::convert::Infallible>().await
        });
        match select(login, select(watch, nudge)).await {
            Either::Left((ran, _)) => First::Ran(ran),
            Either::Right((Either::Left((identity, _)), _)) => First::Confirmed(identity),
            Either::Right((Either::Right((never, _)), _)) => match never {},
        }
    };
    let last = match first {
        First::Confirmed(identity) => signed_in(job, identity),
        First::Ran(Err(_)) => job.failed(SigninCode::Failed),
        First::Ran(Ok(ran)) => match ran.ended {
            Ended::Cancelled => job.failed(SigninCode::Cancelled),
            Ended::TimedOut => job.failed(SigninCode::LinkExpired),
            Ended::Exited(Some(0)) => match status(job, cancel).await {
                Some(Some(identity)) => signed_in(job, identity),
                _ if cancel.is_cancelled() => job.failed(SigninCode::Cancelled),
                _ => job.failed(SigninCode::Failed),
            },
            Ended::Exited(_) if denied.load(Ordering::SeqCst) => job.failed(SigninCode::Cancelled),
            Ended::Exited(_) => job.failed(SigninCode::Failed),
        },
    };
    // The URL file goes before the last state is sent.
    drop(url_file);
    last
}

/// The run's URL file on macOS and Linux, when the worker wrote the shim; none on Windows.
fn url_file<T>(job: &SigninJob<T>) -> std::io::Result<Option<UrlFile>> {
    #[cfg(unix)]
    if job.browser.is_some() {
        return UrlFile::create(&job.config.data_dir, &job.ticket.agent, job.ticket.job).map(Some);
    }
    let _ = job;
    Ok(None)
}

fn lock(file: &Mutex<UrlFile>) -> std::sync::MutexGuard<'_, UrlFile> {
    file.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `SignedIn` for `identity`, Clauseo's own login, now.
fn signed_in<T>(job: &SigninJob<T>, identity: Identity) -> SigninState {
    SigninState::SignedIn {
        email: identity.email,
        plan: identity.plan,
        source: Source::Own,
        at: job.config.clock.now(),
    }
}

/// One `auth status --json` in Clauseo's folder: `Some(Some(identity))` signed in,
/// `Some(None)` signed out, `None` when it could not tell (no start, killed past 5 s, cancelled).
pub(crate) async fn status<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
) -> Option<Option<Identity>> {
    let env = job.env(RunKind::Own { home: &job.home }, None);
    let spec = job.spec(STATUS, env, STATUS_LIMIT);
    let ran = run(
        &*job.launcher,
        &*job.timer,
        spec,
        cancel,
        None,
        |_: &str| {},
    )
    .await
    .ok()?;
    let Ended::Exited(code) = ran.ended else {
        return None;
    };
    let parsed = parse_claude_status(&ran.output.text());
    Some(claude_signed_in(code, parsed.as_ref()))
}

/// One status check: `SignedIn` or `SignedOut`, or no change when it could not tell.
pub(crate) async fn check_status<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
) -> Option<SigninState> {
    let found = status(job, &job.ticket.cancel).await?;
    Some(match found {
        Some(identity) => signed_in(job, identity),
        None => SigninState::SignedOut { found: None },
    })
}

/// `auth logout` in Clauseo's folder, then a status check. Never for a reused login: the call
/// refuses it, and the job checks again here, at the point reuse (S18 piece 5) relies on.
pub(crate) async fn sign_out<T: Timer + Send + Sync>(job: &SigninJob<T>) -> Option<SigninState> {
    if job.shared.signin.source(&job.ticket.agent) == Some(Source::Reused) {
        return None;
    }
    let env = job.env(RunKind::Own { home: &job.home }, None);
    let spec = job.spec(LOGOUT, env, LOGOUT_LIMIT);
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
    if cancel.is_cancelled() {
        return None;
    }
    match status(job, cancel).await {
        Some(Some(identity)) => Some(signed_in(job, identity)),
        Some(None) => Some(SigninState::SignedOut { found: None }),
        // Status could not tell: a logout that exited 0 is taken at its word.
        None => ran
            .ok()
            .filter(|ran| ran.ended == Ended::Exited(Some(0)))
            .map(|_| SigninState::SignedOut { found: None }),
    }
}
