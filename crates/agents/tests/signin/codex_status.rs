//! Codex's status check against definite answers and no answer (reviews H1, G1, L6), and what a
//! login or a sign-out that did not go through leaves (H2, M3, G2), and which accounts sign in
//! (M4); on the same npm install layout as `codex_cli.rs`, whose helpers these share.

#[cfg(unix)]
use clauseo_agents::AgentsEvent;
use clauseo_agents::testing::{hard_link, remove_file, write_file};
use clauseo_agents::{Found, Method, SigninNote, SigninState, Source, Unconfirmed};

#[cfg(unix)]
use crate::codex_cli::plant;
use crate::codex_cli::{failed, home, install, login_source, signed_in, start};
#[cfg(unix)]
use crate::support::{APP_SERVER_LIMIT, CODEX_STATUS_LIMIT, Events, pause};
use crate::support::{CODEX_CLI, Fixture, codex, under};

/// The note a status check that confirmed nothing twice logs: the state is left as it was.
#[cfg(unix)]
fn kept() -> SigninNote {
    SigninNote::StatusNotConfirmed {
        reason: Unconfirmed::Kept,
    }
}

/// Reads Codex's events until `note` arrives and a moment after it, and gives every sign-in
/// state sent meanwhile. For a job that may end with no state at all.
#[cfg(unix)]
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

/// Review G1: a definite `account: null` (with `login status` still saying signed in) signs a
/// `SignedIn` of Clauseo's own out, with no second ask; only no answer keeps the state.
#[test]
fn account_null_signs_an_own_login_out() {
    let fx = Fixture::new("codex-cli-own-null", "ok-after:0");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, signed_in());
    write_file(
        &home(&fx).join("fake-signin-marker"),
        br#"{"authMethod":"no-account"}"#,
    )
    .unwrap();
    let before = fx.codex_runs(&["app-server"]).len();
    agents.check_status(&codex()).unwrap();
    assert_eq!(
        events.until_end().1,
        SigninState::SignedOut {
            found: Some(Found::NoLogin)
        }
    );
    assert_eq!(fx.codex_runs(&["app-server"]).len(), before + 1);
}

/// Review G1: the same for a reused login: `account: null` ends the reuse as signed out.
#[test]
fn account_null_signs_a_reused_login_out() {
    let fx = Fixture::new("codex-cli-reuse-null", "ok-after:0");
    let lawyer = fx.user_home().join(".codex").join("auth.json");
    write_file(&lawyer, b"").unwrap();
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
    // Rewritten in place, so the link still holds.
    write_file(&lawyer, br#"{"authMethod":"no-account"}"#).unwrap();
    agents.check_status(&codex()).unwrap();
    assert_eq!(
        events.until_end().1,
        SigninState::SignedOut {
            found: Some(Found::ReusedSignedOut)
        }
    );
}

/// Review G2: with the lawyer's home folder unknown, the guard cannot tell whether the home's
/// login file is the lawyer's, so a login that did not confirm runs no `logout` and deletes
/// nothing; the sign-in fails as before. (Unix: the Own environment sets `HOME` for the fake.)
#[cfg(unix)]
#[test]
fn with_no_lawyer_home_an_unconfirmed_login_is_not_removed() {
    let mut fx = Fixture::new("codex-cli-no-lawyer-home", "app-server-error");
    fx.config
        .signin
        .base_env
        .remove(std::ffi::OsStr::new("HOME"));
    fx.config
        .signin
        .base_env
        .remove(std::ffi::OsStr::new("USERPROFILE"));
    assert_eq!(fx.config.signin.lawyer_home(), None);
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, failed());
    assert_eq!(fx.codex_runs(&["logout"]), []);
    assert!(home(&fx).join("auth.json").is_file());
}

/// Reads Codex's events until its job ends with no state (`SigninUnchanged`), and gives every
/// sign-in state sent meanwhile.
#[cfg(unix)]
fn states_until_unchanged(events: &Events) -> Vec<SigninState> {
    let mut states = Vec::new();
    for _ in 0..4000 {
        for event in events.drain() {
            match event {
                AgentsEvent::Signin { agent, state } if agent == codex() => states.push(state),
                AgentsEvent::SigninUnchanged { agent } if agent == codex() => return states,
                _ => {}
            }
        }
        pause();
    }
    panic!("never: SigninUnchanged; states {states:?}");
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
    // The job ends with no state, and says so (review L5): `end_job`'s `SigninUnchanged`.
    assert_eq!(states_until_unchanged(&events), []);
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
