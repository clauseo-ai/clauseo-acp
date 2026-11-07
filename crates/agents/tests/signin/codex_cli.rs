//! Codex's sign-in runs the Codex CLI found in the install, and is signed in only on Codex's own
//! confirmation. The fixture lays the install out as npm does: codex-acp's adapter at
//! `node_modules/@agentclientprotocol/codex-acp/dist/index.js`, which `Installed::script` names
//! and the fake plays as the adapter (it ignores the CLI's commands and exits 0 once stdin
//! closes), beside the CLI at `node_modules/@openai/codex/bin/codex.js`.
//!
//! Every test here but the stalled ones runs on Windows too.

use std::path::Path;

use clauseo_agents::testing::{hard_link, remove_file, write_file};
use clauseo_agents::{
    Agents, AgentsEvent, Found, Method, SigninCode, SigninNote, SigninState, Source, Unconfirmed,
};

#[cfg(unix)]
use crate::support::{APP_SERVER_LIMIT, CODEX_STATUS_LIMIT};
use crate::support::{CODEX_ADAPTER, CODEX_CLI, Events, Fixture, codex, now, pause, under};

const EMAIL: &str = "test@example.com";

fn signed_in() -> SigninState {
    SigninState::SignedIn {
        email: Some(EMAIL.to_owned()),
        plan: Some("plus".to_owned()),
        source: Source::Own,
        at: now(),
    }
}

fn failed() -> SigninState {
    SigninState::Failed {
        code: SigninCode::Failed,
        at: now(),
    }
}

fn start(fx: &Fixture) -> (Agents, Events) {
    fx.start_for(codex())
}

fn install(fx: &Fixture) -> std::path::PathBuf {
    fx.data.path().join("codex")
}

fn login_source(fx: &Fixture) -> std::path::PathBuf {
    fx.marker(&codex(), "login-source")
}

/// Codex's home: Clauseo's own `CODEX_HOME`.
fn home(fx: &Fixture) -> std::path::PathBuf {
    fx.config.signin.homes[&codex()].clone()
}

/// The note a status check that confirmed nothing twice logs: the state is left as it was.
fn kept() -> SigninNote {
    SigninNote::StatusNotConfirmed {
        reason: Unconfirmed::Kept,
    }
}

/// Plants the fake's marker in Codex's home: a login already there.
fn plant(fx: &Fixture) {
    let home = home(fx);
    write_file(
        &home.join("fake-signin-marker"),
        br#"{"authMethod":"chatgpt","email":"test@example.com"}"#,
    )
    .unwrap();
}

/// Reads Codex's events until `note` arrives and a moment after it, and gives every sign-in
/// state sent meanwhile. For a job that may end with no state at all.
fn states_through(events: &Events, note: SigninNote) -> Vec<SigninState> {
    let mut states = Vec::new();
    let mut seen = false;
    let mut after = 0;
    for _ in 0..4000 {
        for event in events.drain() {
            match event {
                AgentsEvent::Signin { agent, state } if agent == codex() => states.push(state),
                AgentsEvent::SigninNote { agent, note: got } if agent == codex() && got == note => {
                    seen = true;
                }
                _ => {}
            }
        }
        if seen {
            after += 1;
            if after > 20 {
                return states;
            }
        }
        pause();
    }
    panic!("never: {note:?}; states {states:?}");
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

/// A startup status against the adapter neither signs in nor fails: no state is sent.
#[test]
fn a_startup_status_from_an_adapter_is_neither_signed_in_nor_failed() {
    let fx = Fixture::new("codex-cli-adapter-status", "adapter");
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    let not = SigninNote::StatusNotConfirmed {
        reason: Unconfirmed::NoMarker,
    };
    assert_eq!(states_through(&events, not), []);
    assert_eq!(agents.signin_state(&codex()), None);
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

/// The same errors at a startup status: asked once more, then not `SignedIn`, not `Failed`,
/// nothing sent.
#[test]
fn json_rpc_errors_at_a_startup_status_are_neither_signed_in_nor_failed() {
    let fx = Fixture::new("codex-cli-rpc-error-status", "app-server-error");
    plant(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(states_through(&events, kept()), []);
    assert_eq!(agents.signin_state(&codex()), None);
    assert_eq!(fx.codex_runs(&["app-server"]).len(), 2, "asked twice");
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
/// Codex's own status confirms an account through the CLI: with `account/read` failing, no
/// state is sent.
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
    assert_eq!(states_through(&events, kept()), []);
    assert_eq!(agents.signin_state(&codex()), None);
}

/// The `login status` runs started so far.
#[cfg(unix)]
fn statuses(fx: &Fixture) -> usize {
    fx.codex_runs(&["login", "status"]).len()
}

/// Ends the `count` stalled `login status` runs after the first `before` at their limit, one at
/// a time.
#[cfg(unix)]
fn time_out_statuses(fx: &Fixture, before: usize, count: usize) {
    for n in 1..=count {
        crate::support::eventually("the status ran", || {
            fx.codex_runs(&["login", "status"]).len() >= before + n
        });
        fx.timer.fire(CODEX_STATUS_LIMIT);
    }
}

/// A startup status whose `login status` never answers is killed at its 15 s limit and asked
/// once more; killed again, nothing is sent.
#[cfg(unix)]
#[test]
fn a_startup_status_that_times_out_is_neither_signed_in_nor_failed() {
    let fx = Fixture::new("codex-cli-status-timeout", "ok-after:0");
    plant(&fx);
    let (agents, events) = start(&fx);
    fx.stall(Some("status"));
    let before = statuses(&fx);
    agents.check_status(&codex()).unwrap();
    time_out_statuses(&fx, before, 2);
    assert_eq!(states_through(&events, kept()), []);
    assert_eq!(agents.signin_state(&codex()), None);
    assert_eq!(statuses(&fx), before + 2, "asked twice");
}

/// Review H1: a first status that times out is asked once more, and an answer then counts.
#[cfg(unix)]
#[test]
fn a_status_that_times_out_once_is_asked_again() {
    let fx = Fixture::new("codex-cli-status-retry", "ok-after:0");
    plant(&fx);
    let (agents, events) = start(&fx);
    fx.stall(Some("status"));
    agents.check_status(&codex()).unwrap();
    crate::support::eventually("the status ran", || {
        !fx.codex_runs(&["login", "status"]).is_empty()
    });
    fx.stall(None);
    fx.timer.fire(CODEX_STATUS_LIMIT);
    assert_eq!(events.until_end().1, signed_in());
    assert_eq!(fx.codex_runs(&["login", "status"]).len(), 2);
}

/// Review H1: a fresh install whose status never answers stays signed out: nothing is sent,
/// never `SignedIn`, and no `login-source` is written.
#[cfg(unix)]
#[test]
fn a_fresh_install_whose_status_never_answers_stays_signed_out() {
    let mut fx = Fixture::new("codex-cli-fresh-no-answer", "ok-after:0");
    let ready = fx.not_installed(&codex());
    let (agents, events) = fx.start_missing(codex());
    agents.finish_install_for_tests(&codex(), Some(ready));
    assert!(matches!(
        events.install_state(),
        clauseo_agents::InstallState::Ready { .. }
    ));
    fx.stall(Some("status"));
    let before = statuses(&fx);
    agents.check_status(&codex()).unwrap();
    time_out_statuses(&fx, before, 2);
    assert_eq!(states_through(&events, kept()), []);
    assert_eq!(agents.signin_state(&codex()), None);
    assert!(!login_source(&fx).exists());
}

/// Review H1: a reused login whose background status times out twice is left exactly as it
/// was: no state is sent (so R20's pump neither ends the reuse nor restarts an agent), the
/// `reused` record and the link stay, and the state is still `SignedIn { Reused }`.
#[cfg(unix)]
#[test]
fn a_reused_login_whose_status_times_out_twice_is_left_as_it_was() {
    let fx = Fixture::new("codex-cli-reuse-timeout", "ok-after:0");
    let lawyer = fx.user_home().join(".codex").join("auth.json");
    write_file(&lawyer, b"").unwrap();
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert!(matches!(events.state(), SigninState::LoginFound { .. }));
    agents.use_found_login(&codex()).unwrap();
    let reused = events.state();
    assert!(
        matches!(
            reused,
            SigninState::SignedIn {
                source: Source::Reused,
                ..
            }
        ),
        "{reused:?}"
    );
    let record = std::fs::read_to_string(login_source(&fx)).unwrap();
    fx.stall(Some("status"));
    let before = statuses(&fx);
    agents.check_status(&codex()).unwrap();
    time_out_statuses(&fx, before, 2);
    assert_eq!(states_through(&events, kept()), []);
    assert_eq!(agents.signin_state(&codex()), Some(reused));
    assert_eq!(std::fs::read_to_string(login_source(&fx)).unwrap(), record);
    assert_eq!(
        clauseo_agents::codex_link_holds(&home(&fx), &fx.user_home()),
        clauseo_agents::CodexLink::Holds
    );
    assert!(!fx.marker(&codex(), "no-reuse").exists());
}

/// Review H1: a `SignedIn` whose later status gets no answer (its `account/read` never
/// answers, twice) is left as it was: never `Failed`, never downgraded.
#[cfg(unix)]
#[test]
fn a_signed_in_state_a_later_status_cannot_confirm_is_left_as_it_was() {
    let fx = Fixture::new("codex-cli-unconfirmed", "ok-after:0");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, signed_in());
    let before = fx.codex_runs(&["app-server"]).len();
    fx.stall(Some("app-server"));
    agents.check_status(&codex()).unwrap();
    for n in 1..=2 {
        crate::support::eventually("the app-server ran", || {
            fx.codex_runs(&["app-server"]).len() >= before + n
        });
        fx.timer.fire(APP_SERVER_LIMIT);
    }
    assert_eq!(states_through(&events, kept()), []);
    assert_eq!(agents.signin_state(&codex()), Some(signed_in()));
}

/// A positive "not logged in" still signs a `SignedIn` out at once, with no second ask.
#[test]
fn a_status_that_says_not_logged_in_signs_out() {
    let fx = Fixture::new("codex-cli-said-out", "ok-after:0");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, signed_in());
    remove_file(&home(&fx).join("fake-signin-marker")).unwrap();
    let before = fx.codex_runs(&["login", "status"]).len();
    agents.check_status(&codex()).unwrap();
    assert_eq!(
        events.until_end().1,
        SigninState::SignedOut {
            found: Some(Found::NoLogin)
        }
    );
    // one in Clauseo's home, one for detection
    assert_eq!(fx.codex_runs(&["login", "status"]).len(), before + 2);
}

/// Review H2: the login ran (exit 0, writing Codex's `auth.json`), then `account/read` failed.
/// The sign-in is `signin.failed`, `logout` ran, the home holds no `auth.json`, and the next
/// start's check is signed out, never reused or `CodexLinkSplit`.
#[test]
fn a_login_that_did_not_confirm_leaves_no_login_file() {
    let fx = Fixture::new("codex-cli-failed-cleanup", "app-server-error");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, failed());
    assert_eq!(fx.codex_runs(&["logout"]).len(), 1);
    assert!(std::fs::symlink_metadata(home(&fx).join("auth.json")).is_err());
    assert!(!login_source(&fx).exists());
    drop(agents);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(
        events.until_end().1,
        SigninState::SignedOut {
            found: Some(Found::NoLogin)
        }
    );
}

/// Review M3: with no CLI in the install, a sign-out still removes Clauseo's own login file
/// directly and is `SignedOut`; nothing runs.
#[test]
fn a_sign_out_with_no_cli_removes_the_login_file() {
    let fx = Fixture::new("codex-cli-missing-signout", "ok-after:0");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, signed_in());
    write_file(&home(&fx).join("auth.json"), b"").unwrap();
    remove_file(&under(&install(&fx), CODEX_CLI)).unwrap();
    let before = fx.launches().len();
    agents.sign_out(&codex()).unwrap();
    assert_eq!(events.until_end().1, SigninState::SignedOut { found: None });
    assert!(std::fs::symlink_metadata(home(&fx).join("auth.json")).is_err());
    assert_eq!(fx.launches().len(), before);
}

/// Review M3: with no CLI in the install, "Stop using it here" still removes the link and writes
/// `no-reuse`; nothing runs.
#[test]
fn stop_using_with_no_cli_unlinks_and_marks() {
    let fx = Fixture::new("codex-cli-missing-stop", "ok-after:0");
    write_file(&fx.user_home().join(".codex").join("auth.json"), b"").unwrap();
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
    remove_file(&under(&install(&fx), CODEX_CLI)).unwrap();
    let before = fx.launches().len();
    agents.stop_using_login(&codex(), None).unwrap();
    assert_eq!(
        events.until_end().1,
        SigninState::SignedOut {
            found: Some(Found::ReuseOff)
        }
    );
    assert!(std::fs::symlink_metadata(home(&fx).join("auth.json")).is_err());
    assert!(fx.user_home().join(".codex").join("auth.json").is_file());
    assert!(fx.marker(&codex(), "no-reuse").is_file());
    assert!(!login_source(&fx).exists());
    assert_eq!(fx.launches().len(), before);
}

/// Review M4: the lawyer's `~/.codex` holds an API-key login. It is not a `ChatGPT` sign-in:
/// detection offers nothing (the plain sign-in card), and a reuse of it recorded earlier is
/// signed out at the next start, never "Signed in to `ChatGPT`".
#[test]
fn an_api_key_codex_login_is_never_signed_in_to_chatgpt() {
    let fx = Fixture::new("codex-cli-api-key", "ok-after:0");
    let lawyer = fx.user_home().join(".codex").join("auth.json");
    write_file(&lawyer, br#"{"authMethod": "apikey"}"#).unwrap();
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.until_end().1, SigninState::SignedOut { found: None });
    assert!(agents.use_found_login(&codex()).is_err());
    // A reuse made before this rule: the record and the link (the check made the home).
    hard_link(&lawyer, &home(&fx).join("auth.json")).unwrap();
    write_file(&login_source(&fx), b"reused\n").unwrap();
    agents.check_status(&codex()).unwrap();
    let last = events.until_end().1;
    assert_eq!(
        last,
        SigninState::SignedOut {
            found: Some(Found::ReusedSignedOut)
        }
    );
    assert!(
        events
            .log_notes()
            .contains(&SigninNote::StatusNotConfirmed {
                reason: Unconfirmed::NotChatgpt
            })
    );
    assert!(lawyer.is_file(), "the lawyer's login is never touched");
}
