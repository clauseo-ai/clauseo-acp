//! `codex login` revokes and deletes the login it finds before it starts, so S18 never starts one
//! over Clauseo's own login (`login-source` `own` and the login file in the home) unless the
//! caller passes the lawyer's confirmed switch: `sign_in`, `use_different_account` and
//! `try_again` are refused with `OwnLoginPresent`, `sign_in_switching` and
//! `use_different_account_switching` go ahead. The guard reads the files, so it holds at start-up
//! too, while the state is still `None`.

use clauseo_agents::{Method, SigninError, SigninState};

use crate::codex_cli::{home, start};
use crate::support::{Fixture, codex};

/// Signs Codex in with a login that writes Codex's login file (`slow-save:0`).
fn signed_in_with_file(fx: &Fixture) -> (clauseo_agents::Agents, crate::support::Events) {
    let (agents, events) = start(fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    let last = events.until_end().1;
    assert!(matches!(last, SigninState::SignedIn { .. }), "{last:?}");
    assert!(home(fx).join(concat!("auth", ".json")).is_file());
    (agents, events)
}

fn refused() -> Result<(), SigninError> {
    Err(SigninError::OwnLoginPresent(codex()))
}

#[test]
fn a_login_over_clauseo_s_own_needs_the_confirmed_switch() {
    let fx = Fixture::new("switch-guard", "slow-save:0");
    let (agents, events) = signed_in_with_file(&fx);
    assert!(agents.replaces_own_login(&codex()));
    let before = fx.codex_runs(&["login"]).len();
    assert_eq!(agents.sign_in(&codex(), Method::Browser), refused());
    assert_eq!(agents.sign_in(&codex(), Method::DeviceCode), refused());
    assert_eq!(agents.use_different_account(&codex()), refused());
    assert_eq!(agents.try_again(&codex()), refused());
    assert_eq!(fx.codex_runs(&["login"]).len(), before, "a login ran");
    let reads = fx.codex_runs(&["app-server"]).len();
    agents.sign_in_switching(&codex(), Method::Browser).unwrap();
    // The switch is let through, and the sign-in's quick check finds Clauseo's own login still
    // valid: signed in with it, no login run ("Use different account" always runs a new login).
    // The state names the account `account/read` returned, so the lawyer sees which one it is.
    assert_eq!(events.until_end().1, crate::codex_cli::signed_in());
    assert_eq!(fx.codex_runs(&["login"]).len(), before);
    assert_eq!(fx.codex_runs(&["app-server"]).len(), reads + 1);
}

#[test]
fn the_guard_holds_at_start_up_before_any_state() {
    let fx = Fixture::new("switch-guard-start", "slow-save:0");
    drop(signed_in_with_file(&fx));
    let (agents, _events) = start(&fx);
    assert_eq!(agents.signin_state(&codex()), None);
    assert_eq!(agents.sign_in(&codex(), Method::Browser), refused());
    assert_eq!(agents.use_different_account(&codex()), refused());
}

#[test]
fn with_no_login_file_nothing_is_refused() {
    let fx = Fixture::new("switch-guard-none", "ok-after:0");
    let (agents, events) = start(&fx);
    assert!(!agents.replaces_own_login(&codex()));
    agents.sign_in(&codex(), Method::Browser).unwrap();
    drop(events.until_end());
    // The fake's own marker is not Codex's login file: a second sign-in replaces nothing.
    assert!(!agents.replaces_own_login(&codex()));
    agents.sign_in(&codex(), Method::Browser).unwrap();
}
