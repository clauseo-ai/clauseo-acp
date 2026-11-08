//! Codex's sign-in runs the Codex CLI found in the install, and is signed in only on Codex's own
//! confirmation. The fixture lays the install out as npm does: codex-acp's adapter at
//! `node_modules/@agentclientprotocol/codex-acp/dist/index.js`, which `Installed::script` names
//! and the fake plays as the adapter (it ignores the CLI's commands and exits 0 once stdin
//! closes), beside the CLI at `node_modules/@openai/codex/bin/codex.js`.
//!
//! Every test here runs on Windows too. The status check's answers, and what a login that did not
//! go through leaves, are in `codex_status.rs`.

use std::path::Path;

use clauseo_agents::testing::{remove_file, write_file};
use clauseo_agents::{
    Agents, Found, Method, SigninCode, SigninNote, SigninState, Source, Unconfirmed,
};

use crate::support::{CODEX_ADAPTER, CODEX_CLI, Events, Fixture, codex, now, under};

const EMAIL: &str = "test@example.com";

pub(crate) fn signed_in() -> SigninState {
    SigninState::SignedIn {
        email: Some(EMAIL.to_owned()),
        plan: Some("plus".to_owned()),
        source: Source::Own,
        at: now(),
    }
}

pub(crate) fn failed() -> SigninState {
    SigninState::Failed {
        code: SigninCode::Failed,
        at: now(),
    }
}

pub(crate) fn start(fx: &Fixture) -> (Agents, Events) {
    fx.start_for(codex())
}

pub(crate) fn install(fx: &Fixture) -> std::path::PathBuf {
    fx.data.path().join("codex")
}

pub(crate) fn login_source(fx: &Fixture) -> std::path::PathBuf {
    fx.marker(&codex(), "login-source")
}

/// Codex's home: Clauseo's own `CODEX_HOME`.
pub(crate) fn home(fx: &Fixture) -> std::path::PathBuf {
    fx.config.signin.homes[&codex()].clone()
}

/// Plants the fake's marker in Codex's home: a login already there.
pub(crate) fn plant(fx: &Fixture) {
    let home = home(fx);
    write_file(
        &home.join("fake-signin-marker"),
        br#"{"authMethod":"chatgpt","email":"test@example.com"}"#,
    )
    .unwrap();
}

/// What R20's log line for `note` would hold: its code and detail.
fn log_line(note: SigninNote) -> String {
    format!("note={} detail={:?}", note.code(), note.detail())
}

#[test]
fn a_codex_sign_in_runs_the_cli_from_the_install_never_the_adapter() {
    let fx = Fixture::new("codex-cli-layout", "ok-after:0");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, signed_in());
    let cli = under(&install(&fx), CODEX_CLI);
    let adapter = under(&install(&fx), CODEX_ADAPTER);
    let runs = fx.launches();
    assert_ne!(runs.len(), 0);
    for run in &runs {
        assert_eq!(Path::new(&run.args[0]), cli, "{run:?}");
        assert_ne!(Path::new(&run.args[0]), adapter, "{run:?}");
    }
    assert!(login_source(&fx).is_file());
    let notes = events.log_notes();
    for step in [
        SigninNote::CliResolved { file: "codex.js" },
        SigninNote::LoginExited { code: Some(0) },
        SigninNote::StatusConfirmed,
    ] {
        assert!(notes.contains(&step), "{step:?} in {notes:?}");
    }
    // The log's lines name no account and no path.
    for note in notes {
        let line = log_line(note);
        assert!(!line.contains(EMAIL), "{line}");
        assert!(!line.contains(std::path::MAIN_SEPARATOR), "{line}");
    }
}

/// The adapter answers where the CLI should: the login exits 0 at once, `login status` exits 0
/// and prints nothing. That is no sign-in, and no `login-source` is written.
#[test]
fn an_adapter_that_exits_0_never_signs_in() {
    let fx = Fixture::new("codex-cli-adapter", "adapter");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, failed());
    assert!(!login_source(&fx).exists());
    assert_eq!(fx.codex_runs(&["app-server"]), []);
    let notes = events.log_notes();
    assert!(notes.contains(&SigninNote::LoginExited { code: Some(0) }));
    assert!(notes.contains(&SigninNote::StatusNotConfirmed {
        reason: Unconfirmed::NoMarker
    }));
    assert!(!notes.contains(&SigninNote::StatusConfirmed));
}

/// A startup status against the adapter neither signs in nor fails: its exit 0 with no signed-in
/// line is a definite "not signed in" (review G1), so the card is signed out.
#[test]
fn a_startup_status_from_an_adapter_is_neither_signed_in_nor_failed() {
    let fx = Fixture::new("codex-cli-adapter-status", "adapter");
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(
        events.until_end().1,
        SigninState::SignedOut {
            found: Some(Found::NoLogin)
        }
    );
    assert!(
        events
            .log_notes()
            .contains(&SigninNote::StatusNotConfirmed {
                reason: Unconfirmed::NoMarker
            })
    );
    assert!(!login_source(&fx).exists());
}

/// `login status` says signed in, but `app-server` answers `initialize` with -32602 and
/// `account/read` with -32601, as the adapter does: a lawyer's sign-in fails, and writes no
/// `login-source`.
#[test]
fn json_rpc_errors_from_account_read_fail_a_sign_in() {
    let fx = Fixture::new("codex-cli-rpc-error", "app-server-error");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, failed());
    assert_eq!(fx.codex_runs(&["app-server"]).len(), 1);
    assert!(!login_source(&fx).exists());
    assert!(
        events
            .log_notes()
            .contains(&SigninNote::StatusNotConfirmed {
                reason: Unconfirmed::NoAccount
            })
    );
}

/// The same errors at a startup status: a definite "no account" (review G1), so signed out at
/// once, never `SignedIn` and never `Failed`.
#[test]
fn json_rpc_errors_at_a_startup_status_are_neither_signed_in_nor_failed() {
    let fx = Fixture::new("codex-cli-rpc-error-status", "app-server-error");
    plant(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(
        events.until_end().1,
        SigninState::SignedOut {
            found: Some(Found::NoLogin)
        }
    );
    assert_eq!(fx.codex_runs(&["app-server"]).len(), 1, "asked once");
}

/// No `codex.js` in the install: a sign-in fails at once and runs nothing (the adapter least of
/// all); a status check reads as signed out, never `Failed`.
#[test]
fn a_missing_codex_cli_fails_the_sign_in_and_runs_nothing() {
    let fx = Fixture::new("codex-cli-missing", "ok-after:0");
    remove_file(&under(&install(&fx), CODEX_CLI)).unwrap();
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, failed());
    assert_eq!(fx.launches().len(), 0);
    assert!(events.notes().contains(&SigninNote::CliMissing));
    assert!(!login_source(&fx).exists());
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.until_end().1, SigninState::SignedOut { found: None });
    assert_eq!(fx.launches().len(), 0);
}

/// A startup status with no login anywhere is the sign-in card (signed out), never `Failed`.
#[test]
fn a_startup_status_with_no_login_is_signed_out() {
    let fx = Fixture::new("codex-cli-no-login", "ok-after:0");
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(
        events.until_end().1,
        SigninState::SignedOut {
            found: Some(Found::NoLogin)
        }
    );
    assert!(
        events
            .log_notes()
            .contains(&SigninNote::StatusNotConfirmed {
                reason: Unconfirmed::SignedOut
            })
    );
}

/// A fresh install with no login stays "Sign in": the status check R20 runs once the install is
/// `Ready` (and at every start) goes through the CLI and is signed out, never `SignedIn` (the
/// adapter's `login status` exits 0) and never `Failed`, and writes no `login-source`.
#[test]
fn a_fresh_install_with_no_login_stays_signed_out() {
    let mut fx = Fixture::new("codex-cli-fresh", "ok-after:0");
    let ready = fx.not_installed(&codex());
    let (agents, events) = fx.start_missing(codex());
    agents.finish_install_for_tests(&codex(), Some(ready));
    assert!(matches!(
        events.install_state(),
        clauseo_agents::InstallState::Ready { .. }
    ));
    agents.check_status(&codex()).unwrap();
    assert_eq!(
        events.until_end().1,
        SigninState::SignedOut {
            found: Some(Found::NoLogin)
        }
    );
    let cli = under(&install(&fx), CODEX_CLI);
    assert!(
        fx.launches()
            .iter()
            .all(|run| Path::new(&run.args[0]) == cli)
    );
    assert!(!login_source(&fx).exists());
    assert_eq!(
        agents.signin_state(&codex()),
        Some(SigninState::SignedOut {
            found: Some(Found::NoLogin)
        })
    );
}

/// A reused login (the record and the link both there) is signed in at a later start only once
/// Codex's own status confirms an account through the CLI: with `account/read` answering
/// JSON-RPC errors, the reuse ends as signed out.
#[test]
fn a_reused_login_needs_codex_s_own_confirmation_at_a_later_start() {
    let mut fx = Fixture::new("codex-cli-reuse", "ok-after:0");
    write_file(&fx.user_home().join(".codex").join("auth.json"), b"").unwrap();
    {
        let (agents, events) = start(&fx);
        agents.check_status(&codex()).unwrap();
        assert!(matches!(events.state(), SigninState::LoginFound { .. }));
        agents.use_found_login(&codex()).unwrap();
        assert!(matches!(
            events.state(),
            SigninState::SignedIn {
                source: Source::Reused,
                ..
            }
        ));
        // Confirmed at a later start while the CLI says so.
        agents.check_status(&codex()).unwrap();
        assert!(matches!(
            events.until_end().1,
            SigninState::SignedIn {
                source: Source::Reused,
                ..
            }
        ));
    }
    fx.config
        .signin
        .base_env
        .insert("FAKE_SIGNIN".into(), "app-server-error".into());
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(
        events.until_end().1,
        SigninState::SignedOut {
            found: Some(Found::ReusedSignedOut)
        }
    );
}
