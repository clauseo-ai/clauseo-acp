//! The status check that times out at start (the Windows cold boot whose first `auth status`
//! was killed and left the card with nothing on it): `Checking` while it runs, a first run with
//! a 15 s limit, one retry with a 20 s limit, then Sign in with re-checks in the background; and
//! the quick status check a sign-in makes first (5 s, `Checking` while it runs).

#[cfg(unix)]
use std::time::Duration;

use clauseo_agents::testing::write_file;
use clauseo_agents::{Method, SigninState, Source};

use crate::support::{Fixture, Seen, claude, now};
#[cfg(unix)]
use crate::support::{STATUS_LIMIT, eventually};

/// The start-up check's first run's limit (`found::FIRST_LIMIT`).
#[cfg(unix)]
const FIRST_LIMIT: Duration = Duration::from_secs(15);
/// The one retry's limit (`found::RETRY_LIMIT`).
#[cfg(unix)]
const RETRY_LIMIT: Duration = Duration::from_secs(20);
#[cfg(unix)]
const FIRST_RECHECK: Duration = Duration::from_secs(30);

fn signed_in() -> SigninState {
    SigninState::SignedIn {
        email: Some("test@example.com".to_owned()),
        plan: Some("max".to_owned()),
        source: Source::Own,
        at: now(),
    }
}

/// Claude's own login, already there in Clauseo's folder.
fn plant_login(fx: &Fixture) {
    let home = fx.config.signin.homes[&claude()].clone();
    write_file(
        &home.join("fake-signin-marker"),
        br#"{"authMethod":"claude.ai","email":"test@example.com"}"#,
    )
    .unwrap();
}

/// Waits for the `n`th `auth status` run, then ends it at `limit`.
#[cfg(unix)]
fn time_out_status(fx: &Fixture, n: usize, limit: Duration) {
    eventually("the status ran", || fx.runs_of("status").len() >= n);
    fx.timer.fire(limit);
}

/// (a), (d): a status that never answers shows `Checking`, its first run has a 15 s limit, then
/// one retry with a 20 s limit, and then shows Sign in (`SignedOut`), with a re-check armed at
/// 30 s. No 5 s limit is armed for the start-up check.
#[cfg(unix)]
#[test]
fn a_status_that_times_out_twice_shows_checking_then_sign_in() {
    let fx = Fixture::new("status-retry-twice", "never");
    let (agents, events) = fx.start();
    fx.stall(Some("status"));
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.next(), Seen::State(SigninState::Checking));
    assert_eq!(agents.signin_state(&claude()), Some(SigninState::Checking));
    time_out_status(&fx, 1, FIRST_LIMIT);
    // Still checking through the retry: nothing else was sent.
    assert_eq!(agents.signin_state(&claude()), Some(SigninState::Checking));
    time_out_status(&fx, 2, RETRY_LIMIT);
    assert_eq!(
        events.next(),
        Seen::State(SigninState::SignedOut { found: None })
    );
    assert_eq!(fx.runs_of("status").len(), 2);
    let armed = fx.timer.armed();
    for limit in [FIRST_LIMIT, RETRY_LIMIT] {
        assert!(armed.contains(&limit), "{limit:?} in {armed:?}");
    }
    assert!(!armed.contains(&STATUS_LIMIT), "{armed:?}");
    eventually("the re-check is armed", || {
        fx.timer.live().contains(&FIRST_RECHECK)
    });
}

/// (b): the first status times out and the retry answers signed in: the card ends signed in.
#[cfg(unix)]
#[test]
fn a_retry_that_answers_signs_in() {
    let fx = Fixture::new("status-retry-answers", "never");
    plant_login(&fx);
    let (agents, events) = fx.start();
    fx.stall(Some("status"));
    agents.check_status(&claude()).unwrap();
    eventually("the status ran", || fx.runs_of("status").len() == 1);
    fx.stall(None);
    fx.timer.fire(FIRST_LIMIT);
    let (seen, last) = events.until_end();
    assert_eq!(last, signed_in());
    assert_eq!(seen.first(), Some(&Seen::State(SigninState::Checking)));
    assert_eq!(fx.runs_of("status").len(), 2);
    assert!(fx.timer.armed().contains(&RETRY_LIMIT));
}

/// (c): Sign in shown after the timeouts, then a background re-check answers signed in: the
/// state is corrected.
#[cfg(unix)]
#[test]
fn a_background_recheck_corrects_sign_in_to_signed_in() {
    let fx = Fixture::new("status-retry-recheck", "never");
    let (agents, events) = fx.start();
    fx.stall(Some("status"));
    agents.check_status(&claude()).unwrap();
    time_out_status(&fx, 1, FIRST_LIMIT);
    time_out_status(&fx, 2, RETRY_LIMIT);
    assert_eq!(events.until_end().1, SigninState::SignedOut { found: None });
    plant_login(&fx);
    fx.stall(None);
    fx.timer.fire(FIRST_RECHECK);
    assert_eq!(events.until_end().1, signed_in());
    assert_eq!(agents.signin_state(&claude()), Some(signed_in()));
}

/// (e): Sign in pressed while Clauseo's own login is already there: the quick status check says
/// signed in, so no login runs and no page opens.
#[test]
fn sign_in_with_a_login_already_there_runs_no_login() {
    let fx = Fixture::new("status-quick-check", "never");
    plant_login(&fx);
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    let (seen, last) = events.until_end();
    assert_eq!(last, signed_in());
    assert_eq!(seen.first(), Some(&Seen::State(SigninState::Checking)));
    assert_eq!(fx.runs_of("login"), []);
    assert_eq!(fx.runs_of("status").len(), 1);
    assert!(
        !seen.iter().any(|seen| matches!(seen, Seen::Open(_))),
        "{seen:?}"
    );
    assert!(
        !seen
            .iter()
            .any(|seen| matches!(seen, Seen::State(SigninState::Waiting { .. }))),
        "{seen:?}"
    );
}

/// (f): Sign in pressed and the quick status check is slow: `Checking` is published while it
/// runs, before any result. Its 5 s limit passes without an answer, so the sign-in goes on as
/// usual: the login runs and the card moves on to `Waiting`, with no `SignedIn` on the way.
#[cfg(unix)]
#[test]
fn a_slow_quick_check_shows_checking_then_the_sign_in_flow() {
    let fx = Fixture::new("status-quick-slow", "never");
    let (agents, events) = fx.start();
    fx.stall(Some("status"));
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    eventually("the quick check ran", || fx.runs_of("status").len() == 1);
    // Published before the check ran, so already there: the state, then its event.
    assert_eq!(agents.signin_state(&claude()), Some(SigninState::Checking));
    assert_eq!(events.next(), Seen::State(SigninState::Checking));
    assert_eq!(fx.runs_of("login"), []);
    fx.stall(None);
    fx.timer.fire(STATUS_LIMIT);
    eventually("the login started", || fx.runs_of("login").len() == 1);
    let seen = events.until(|state| matches!(state, SigninState::Waiting { .. }));
    assert!(
        !seen
            .iter()
            .any(|seen| matches!(seen, Seen::State(SigninState::SignedIn { .. }))),
        "{seen:?}"
    );
    assert_eq!(fx.runs_of("status").len(), 1);
    assert!(matches!(
        agents.signin_state(&claude()),
        Some(SigninState::Waiting { .. })
    ));
    agents.cancel_sign_in(&claude()).unwrap();
}
