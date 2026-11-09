//! The status check that times out at start (the Windows cold boot whose first `auth status`
//! was killed at 5 s and left the card with nothing on it): `Checking` while it runs, a retry
//! with a 10 s limit, then a 20 s one, then Sign in with re-checks in the background; and the
//! quick status check a sign-in makes first.

#[cfg(unix)]
use std::time::Duration;

use clauseo_agents::testing::write_file;
use clauseo_agents::{Method, SigninState, Source};

use crate::support::{Fixture, Seen, claude, now};
#[cfg(unix)]
use crate::support::{STATUS_LIMIT, STATUS_RUNS, eventually};

#[cfg(unix)]
const RETRY: Duration = Duration::from_secs(10);
#[cfg(unix)]
const LAST_RETRY: Duration = Duration::from_secs(20);
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

/// Waits for the `n`th check's `auth status` runs (the own status and detection beside it,
/// [`STATUS_RUNS`]), then ends them at `limit`.
#[cfg(unix)]
fn time_out_status(fx: &Fixture, n: usize, limit: Duration) {
    eventually("the status ran", || {
        fx.runs_of("status").len() >= n * STATUS_RUNS
    });
    for _ in 0..STATUS_RUNS {
        fx.timer.fire(limit);
    }
}

/// (a), (d): a status that never answers shows `Checking`, is asked again with a 10 s limit,
/// then a 20 s one, and then shows Sign in (`SignedOut`), with a re-check armed at 30 s.
#[cfg(unix)]
#[test]
fn a_status_that_times_out_thrice_shows_checking_then_sign_in() {
    let fx = Fixture::new("status-retry-thrice", "never");
    let (agents, events) = fx.start();
    fx.stall(Some("status"));
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.next(), Seen::State(SigninState::Checking));
    assert_eq!(agents.signin_state(&claude()), Some(SigninState::Checking));
    time_out_status(&fx, 1, STATUS_LIMIT);
    time_out_status(&fx, 2, RETRY);
    // Still checking through the retries: nothing else was sent.
    assert_eq!(agents.signin_state(&claude()), Some(SigninState::Checking));
    time_out_status(&fx, 3, LAST_RETRY);
    assert_eq!(
        events.next(),
        Seen::State(SigninState::SignedOut { found: None })
    );
    assert_eq!(fx.runs_of("status").len(), 3 * STATUS_RUNS);
    let armed = fx.timer.armed();
    for limit in [STATUS_LIMIT, RETRY, LAST_RETRY] {
        assert!(armed.contains(&limit), "{limit:?} in {armed:?}");
    }
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
    eventually("the status ran", || {
        fx.runs_of("status").len() == STATUS_RUNS
    });
    fx.stall(None);
    for _ in 0..STATUS_RUNS {
        fx.timer.fire(STATUS_LIMIT);
    }
    let (seen, last) = events.until_end();
    assert_eq!(last, signed_in());
    assert_eq!(seen.first(), Some(&Seen::State(SigninState::Checking)));
    assert_eq!(fx.runs_of("status").len(), 2 * STATUS_RUNS);
    assert!(fx.timer.armed().contains(&RETRY));
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
    time_out_status(&fx, 1, STATUS_LIMIT);
    time_out_status(&fx, 2, RETRY);
    time_out_status(&fx, 3, LAST_RETRY);
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
