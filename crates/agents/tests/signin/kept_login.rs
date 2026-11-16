//! A Codex login already in Clauseo's home is never removed by a sign-in that did not write it:
//! a Cancel or the quit while the sign-in's quick check runs (`Checking`) ends the sign-in there,
//! with no login started; and a login that ran but left the file as it was does not remove it
//! when the check after it cannot confirm. A login that replaced the file and did not confirm
//! still goes.

use std::path::PathBuf;
use std::time::Duration;

use clauseo_agents::testing::write_file;
use clauseo_agents::{Method, SigninCode, SigninState};

use crate::codex_cli::{home, login_source, start};
use crate::support::{Fixture, Seen, codex, eventually, now};

/// The quit's bound, as the desktop app gives it (`AGENTS_STOP_WAIT`).
const QUIT_WAIT: Duration = Duration::from_secs(3);
/// The most one `app-server` account read may take (`identity::APP_SERVER_LIMIT`).
const APP_SERVER_LIMIT: Duration = Duration::from_secs(20);

fn login_file(fx: &Fixture) -> PathBuf {
    home(fx).join(concat!("auth", ".json"))
}

fn states(seen: &[Seen]) -> Vec<SigninState> {
    seen.iter()
        .filter_map(|seen| match seen {
            Seen::State(state) => Some(state.clone()),
            Seen::Open(_) => None,
        })
        .collect()
}

fn failed(code: SigninCode) -> SigninState {
    SigninState::Failed { code, at: now() }
}

/// Clauseo's own login from before: `login-source` `own`, and a dummy login file (never a
/// credential) with `contents`, which the fake reads as its marker.
fn own_login(fx: &Fixture, contents: &[u8]) {
    write_file(&login_source(fx), b"own\n").unwrap();
    write_file(&login_file(fx), contents).unwrap();
}

/// An own login the fake's `login status` says signed in to, whose account read says signed out
/// (`account: null`): any confirm of it says signed out.
const NO_ACCOUNT: &[u8] = br#"{"authMethod":"no-account","email":"test@example.com"}"#;

/// The lawyer's Cancel while the quick check's `login status` runs: `Failed { Cancelled }`
/// straight after `Checking`, with no `Waiting`, no login run, no status run but the quick
/// check's own, no logout, and the login from before untouched, though a confirm of it would
/// say signed out.
#[test]
fn a_cancel_while_checking_starts_no_login_and_keeps_the_login_there() {
    let mut fx = Fixture::new("kept-cancel-checking", "never");
    fx.set_env("FAKE_STATUS_PAUSE_MS", "800");
    own_login(&fx, NO_ACCOUNT);
    let (agents, events) = start(&fx);
    agents.sign_in_switching(&codex(), Method::Browser).unwrap();
    eventually("the quick check's status runs", || {
        !fx.codex_runs(&["login", "status"]).is_empty()
    });
    agents.cancel_sign_in(&codex()).unwrap();
    let (seen, last) = events.until_end();
    assert_eq!(last, failed(SigninCode::Cancelled));
    assert_eq!(
        states(&seen),
        [SigninState::Checking, failed(SigninCode::Cancelled)]
    );
    assert_eq!(fx.codex_runs(&["login"]), [], "a login ran");
    assert_eq!(fx.codex_runs(&["login", "status"]).len(), 1);
    assert_eq!(fx.codex_runs(&["app-server"]), []);
    assert_eq!(fx.codex_runs(&["logout"]), []);
    assert_eq!(std::fs::read(login_file(&fx)).unwrap(), NO_ACCOUNT);
}

/// Claude: the lawyer's Cancel while the quick check's `auth status` runs (stalled, so it is
/// killed when the lawyer's grace passes) ends `Failed { Cancelled }` straight after
/// `Checking`: no `Waiting`, and no login run.
#[cfg(unix)]
#[test]
fn a_claude_cancel_while_checking_starts_no_login() {
    let fx = Fixture::new("kept-claude-cancel-checking", "never");
    fx.stall(Some("status"));
    let claude = crate::support::claude();
    let (agents, events) = fx.start_for(claude.clone());
    agents.sign_in(&claude, Method::ClaudeAi).unwrap();
    eventually("the quick check's status runs", || {
        !fx.runs_of("status").is_empty()
    });
    agents.cancel_sign_in(&claude).unwrap();
    fx.timer.fire(Duration::from_secs(2));
    let (seen, last) = events.until_end();
    assert_eq!(last, failed(SigninCode::Cancelled));
    assert_eq!(
        states(&seen),
        [SigninState::Checking, failed(SigninCode::Cancelled)]
    );
    assert_eq!(fx.runs_of("login"), [], "a login ran");
    assert_eq!(fx.runs_of("status").len(), 1);
}

/// The quit while the quick check's `login status` runs: it ends inside its bound, with no
/// login run, no logout, and the login from before untouched.
#[test]
fn the_quit_while_checking_starts_no_login_and_keeps_the_login_there() {
    let mut fx = Fixture::new("kept-quit-checking", "never");
    fx.set_env("FAKE_STATUS_PAUSE_MS", "800");
    own_login(&fx, NO_ACCOUNT);
    let (agents, _events) = start(&fx);
    agents.sign_in_switching(&codex(), Method::Browser).unwrap();
    eventually("the quick check's status runs", || {
        !fx.codex_runs(&["login", "status"]).is_empty()
    });
    assert!(agents.shutdown(QUIT_WAIT), "a job still runs");
    assert_eq!(fx.codex_runs(&["login"]), [], "a login ran");
    assert_eq!(fx.codex_runs(&["logout"]), []);
    assert_eq!(std::fs::read(login_file(&fx)).unwrap(), NO_ACCOUNT);
}

/// (1) A login ran and saved another login file over the one from before, and Codex does not
/// confirm it (every account read is a JSON-RPC error: signed out): it goes, as before.
#[test]
fn a_login_that_replaced_the_file_and_did_not_confirm_removes_it() {
    let fx = Fixture::new("kept-replaced", "app-server-error");
    own_login(&fx, br#"{"authMethod":"before"}"#);
    let (agents, events) = start(&fx);
    agents.sign_in_switching(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, failed(SigninCode::Failed));
    assert_eq!(fx.codex_runs(&["login"]).len(), 1);
    assert!(!login_file(&fx).exists(), "the unconfirmed login stayed");
}

/// (3) A login ran and ended without writing (the lawyer refused in the browser), the file from
/// before is as it was, and the check after it gets no answer (the account read is killed at its
/// limit): the file stays, and no logout runs.
#[test]
fn a_login_that_left_the_file_as_it_was_keeps_it_when_the_check_cannot_tell() {
    let mut fx = Fixture::new("kept-unchanged-unknown", "cancel");
    fx.set_env("FAKE_APP_SERVER_PAUSE_MS", "60000");
    let before = br#"{"authMethod":"chatgpt","email":"test@example.com"}"#;
    own_login(&fx, before);
    let (agents, events) = start(&fx);
    agents.sign_in_switching(&codex(), Method::Browser).unwrap();
    // The quick check's account read, then the check's after the login: no answer, either.
    fx.timer.fire(APP_SERVER_LIMIT);
    eventually("the login ran", || !fx.codex_runs(&["login"]).is_empty());
    fx.timer.fire(APP_SERVER_LIMIT);
    assert_eq!(events.until_end().1, failed(SigninCode::Cancelled));
    assert_eq!(fx.codex_runs(&["login"]).len(), 1);
    assert_eq!(fx.codex_runs(&["app-server"]).len(), 2);
    assert_eq!(fx.codex_runs(&["logout"]), []);
    assert_eq!(std::fs::read(login_file(&fx)).unwrap(), before);
}
