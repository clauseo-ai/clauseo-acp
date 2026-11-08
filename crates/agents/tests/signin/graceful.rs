//! Quit and Cancel stop Codex's sign-in runs gracefully (`signin/stopping.rs`): the browser
//! login gets `GET /cancel` on its callback port and ends writing nothing; a login in the middle
//! of saving Codex's login file finishes the save, so the file is whole; a status is awaited; a
//! run still going at the quit's bound is killed; and a 0-byte login file in Clauseo's home is
//! removed before a status reads it; the quit's kill and a second stop end the check after a
//! Cancel without a logout. No test fires the lawyer's 2 s grace: every run ends by itself.

use std::path::PathBuf;
use std::time::Duration;

use clauseo_agents::testing::write_file;
use clauseo_agents::{Method, SigninCode, SigninState};

use crate::codex_cli::{home, start};
use crate::support::{Fixture, codex, eventually, now};

/// The quit's bound, as the desktop app gives it (`AGENTS_STOP_WAIT`).
const QUIT_WAIT: Duration = Duration::from_secs(3);

fn login_file(fx: &Fixture) -> PathBuf {
    home(fx).join(concat!("auth", ".json"))
}

/// Starts Codex's browser login on the fake (`script`) with its callback on a port of the
/// test's own (`Fixture::listen_for_cancel`), and waits until it listens there: the fake's
/// `fake-login-listening` file, written once it is bound. No connection probes the port: one
/// that comes before the login binds it is refused only after about 2 s on Windows.
fn login_listening(fx: &Fixture) -> (clauseo_agents::Agents, crate::support::Events) {
    let (agents, events) = start(fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    let listening = home(fx).join("fake-login-listening");
    eventually("the login listens", || listening.is_file());
    (agents, events)
}

/// The lawyer's Cancel: `/cancel` reaches the login, which exits writing nothing; Codex's status
/// then says signed out, so the state is `Failed { Cancelled }` (the signed-out card), never the
/// `SignedIn` from before.
#[test]
fn cancel_sends_cancel_and_the_login_ends_writing_nothing() {
    let mut fx = Fixture::new("graceful-cancel", "never");
    fx.listen_for_cancel();
    let (agents, events) = login_listening(&fx);
    let login = fx.codex_runs(&["login"]).remove(0);
    agents.cancel_sign_in(&codex()).unwrap();
    let (seen, last) = events.until_end();
    assert_eq!(
        last,
        SigninState::Failed {
            code: SigninCode::Cancelled,
            at: now()
        },
        "{seen:?}"
    );
    assert!(
        home(&fx).join("fake-login-cancelled").is_file(),
        "no /cancel came"
    );
    assert!(!login_file(&fx).exists());
    assert_eq!(
        fx.codex_runs(&["login", "status"]).len(),
        1,
        "one check after"
    );
    #[cfg(target_os = "linux")]
    assert!(crate::support::gone(login.pid), "pid {} runs", login.pid);
    let _ = login;
}

/// The quit: the same `/cancel`, inside the bound, and no check or logout after it.
#[test]
fn quit_sends_cancel_and_ends_inside_its_bound() {
    let mut fx = Fixture::new("graceful-quit", "never");
    fx.listen_for_cancel();
    let (agents, _events) = login_listening(&fx);
    let login = fx.codex_runs(&["login"]).remove(0);
    assert!(agents.shutdown(QUIT_WAIT), "a job still runs");
    assert!(
        home(&fx).join("fake-login-cancelled").is_file(),
        "no /cancel came"
    );
    assert!(!login_file(&fx).exists());
    assert_eq!(fx.codex_runs(&["login", "status"]), []);
    assert_eq!(fx.codex_runs(&["logout"]), []);
    #[cfg(target_os = "linux")]
    assert!(crate::support::gone(login.pid), "pid {} runs", login.pid);
    let _ = login;
}

/// The quit while the login saves Codex's login file (truncated, then written in two halves a
/// second apart): the `/cancel` waits behind the save and the save runs to its end, never cut.
/// The saved login was never confirmed and has no `login-source` record, which at the next start
/// would read as reused, so the quit removes it: the home holds no unmarked login file.
#[test]
fn a_quit_in_the_middle_of_a_save_lets_it_finish_and_leaves_no_unmarked_file() {
    let mut fx = Fixture::new("graceful-mid-write", "slow-save:1000");
    fx.listen_for_cancel();
    let (agents, _events) = login_listening(&fx);
    eventually("the save started", || login_file(&fx).exists());
    assert!(agents.shutdown(QUIT_WAIT), "a job still runs");
    assert!(
        home(&fx).join("fake-save-finished").is_file(),
        "the save was cut"
    );
    assert!(!fx.marker(&codex(), "login-source").exists());
    assert!(!login_file(&fx).exists(), "an unmarked login file is left");
}

/// The stop at once is not held up by a `/cancel` the login does not answer (it is saving):
/// with a bound the request's 1 s read would pass, the quit still kills the login inside it,
/// and the cut file, unmarked, goes.
#[test]
fn a_slow_cancel_does_not_delay_the_kill_at_the_bound() {
    let mut fx = Fixture::new("graceful-slow-cancel", "slow-save:3000");
    fx.listen_for_cancel();
    let (agents, _events) = login_listening(&fx);
    eventually("the save started", || login_file(&fx).exists());
    let login = fx.codex_runs(&["login"]).remove(0);
    assert!(
        agents.shutdown(Duration::from_millis(900)),
        "the kill came late"
    );
    assert!(!home(&fx).join("fake-save-finished").exists());
    assert!(!login_file(&fx).exists(), "an unmarked login file is left");
    #[cfg(target_os = "linux")]
    assert!(crate::support::gone(login.pid), "pid {} runs", login.pid);
    let _ = login;
}

/// A login whose confirmation lands as the quit starts (the account read answers after it) is
/// recorded as Clauseo's own, so the next start reads it as own, not reused.
#[test]
fn a_confirmation_during_the_quit_is_recorded_as_own() {
    let mut fx = Fixture::new("graceful-confirm-quit", "slow-save:0");
    fx.set_env("FAKE_APP_SERVER_PAUSE_MS", "500");
    {
        let (agents, _events) = start(&fx);
        agents.sign_in(&codex(), Method::Browser).unwrap();
        eventually("the account read runs", || {
            !fx.codex_runs(&["app-server"]).is_empty()
        });
        assert!(agents.shutdown(QUIT_WAIT), "a job still runs");
    }
    let source = std::fs::read_to_string(fx.marker(&codex(), "login-source")).unwrap();
    assert!(source.contains("own"), "{source}");
    assert!(login_file(&fx).is_file());
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    let last = events.until_end().1;
    assert!(
        matches!(
            last,
            SigninState::SignedIn {
                source: clauseo_agents::Source::Own,
                ..
            }
        ),
        "{last:?}"
    );
}

/// Signs Codex in with Clauseo's own login (`login-source` `own`), so a status check reads
/// Clauseo's home.
fn signed_in_own(fx: &Fixture) -> (clauseo_agents::Agents, crate::support::Events) {
    let (agents, events) = start(fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    let last = events.until_end().1;
    assert!(matches!(last, SigninState::SignedIn { .. }), "{last:?}");
    (agents, events)
}

/// Starts a status check and waits until its `login status` runs.
fn status_running(fx: &Fixture, agents: &clauseo_agents::Agents) {
    let before = fx.codex_runs(&["login", "status"]).len();
    agents.check_status(&codex()).unwrap();
    eventually("a status runs", || {
        fx.codex_runs(&["login", "status"]).len() > before
    });
}

/// The quit while a status runs: it is awaited, not killed, and the login file is as it was.
#[test]
fn a_quit_waits_for_a_running_status() {
    let mut fx = Fixture::new("graceful-status", "ok-after:0");
    fx.set_env("FAKE_STATUS_PAUSE_MS", "400");
    let (agents, _events) = signed_in_own(&fx);
    // The sign-in's own status checks left the file; only the quit's status may leave it now.
    let finished = home(&fx).join("fake-status-finished");
    clauseo_agents::testing::remove_file(&finished).unwrap();
    let before = br#"{"authMethod":"chatgpt","email":"test@example.com"}"#;
    write_file(&login_file(&fx), before).unwrap();
    status_running(&fx, &agents);
    assert!(agents.shutdown(QUIT_WAIT), "a job still runs");
    assert!(finished.is_file(), "the status was killed");
    assert_eq!(std::fs::read(login_file(&fx)).unwrap(), before);
}

/// A 0-byte login file in Clauseo's home (a save a crash cut) is removed before a status reads
/// it, and the status then says signed out; a file with contents is left alone.
#[test]
fn an_empty_login_file_is_removed_before_a_status() {
    let fx = Fixture::new("graceful-empty", "ok-after:0");
    let (agents, events) = signed_in_own(&fx);
    clauseo_agents::testing::remove_file(&home(&fx).join("fake-signin-marker")).unwrap();
    write_file(&login_file(&fx), b"").unwrap();
    agents.check_status(&codex()).unwrap();
    let last = events.until_end().1;
    assert!(matches!(last, SigninState::SignedOut { .. }), "{last:?}");
    assert!(!login_file(&fx).exists());

    let full = Fixture::new("graceful-full", "ok-after:0");
    let (agents, _events) = signed_in_own(&full);
    let contents = br#"{"authMethod":"chatgpt","email":"test@example.com"}"#;
    write_file(&login_file(&full), contents).unwrap();
    status_running(&full, &agents);
    assert!(agents.shutdown(QUIT_WAIT));
    assert_eq!(std::fs::read(login_file(&full)).unwrap(), contents);
}

/// The quit's kill cuts a confirmation (an account read paused past the bound): the unconfirmed,
/// unmarked login file is removed directly and no logout starts, since none could finish before
/// the app exits.
#[test]
fn a_confirmation_cut_by_the_kill_leaves_no_file_and_starts_no_logout() {
    let mut fx = Fixture::new("graceful-cut-confirm", "slow-save:0");
    fx.set_env("FAKE_APP_SERVER_PAUSE_MS", "5000");
    let (agents, _events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    eventually("the account read runs", || {
        !fx.codex_runs(&["app-server"]).is_empty()
    });
    assert!(
        agents.shutdown(Duration::from_millis(1500)),
        "a job still runs"
    );
    assert_eq!(fx.codex_runs(&["logout"]), []);
    assert!(!fx.marker(&codex(), "login-source").exists());
    assert!(!login_file(&fx).exists(), "an unmarked login file is left");
}

/// The lawyer's Cancel while the login saves, then the quit while the check after it confirms:
/// the confirmation that lands is recorded as Clauseo's own.
#[test]
fn cancel_then_quit_records_a_confirmation_that_lands() {
    let mut fx = Fixture::new("graceful-cancel-quit-confirm", "slow-save:1000");
    fx.set_env("FAKE_APP_SERVER_PAUSE_MS", "1000");
    fx.listen_for_cancel();
    let (agents, _events) = login_listening(&fx);
    eventually("the save started", || login_file(&fx).exists());
    agents.cancel_sign_in(&codex()).unwrap();
    eventually("the check's account read runs", || {
        !fx.codex_runs(&["app-server"]).is_empty()
    });
    assert!(agents.shutdown(QUIT_WAIT), "a job still runs");
    let source = std::fs::read_to_string(fx.marker(&codex(), "login-source")).unwrap();
    assert!(source.contains("own"), "{source}");
    assert!(login_file(&fx).is_file());
}

/// After the lawyer's Cancel a status check follows; a second Cancel stops it: the status
/// running is awaited, and nothing more starts (no logout).
#[test]
fn a_second_cancel_stops_the_check_after_the_first() {
    let mut fx = Fixture::new("graceful-cancel-twice", "never");
    fx.set_env("FAKE_STATUS_PAUSE_MS", "800");
    fx.listen_for_cancel();
    let (agents, events) = login_listening(&fx);
    agents.cancel_sign_in(&codex()).unwrap();
    eventually("the check's status runs", || {
        !fx.codex_runs(&["login", "status"]).is_empty()
    });
    agents.cancel_sign_in(&codex()).unwrap();
    let last = events.until_end().1;
    let cancelled = SigninState::Failed {
        code: SigninCode::Cancelled,
        at: now(),
    };
    assert_eq!(last, cancelled);
    assert_eq!(fx.codex_runs(&["logout"]), [], "the check went on");
    assert_eq!(fx.codex_runs(&["app-server"]), []);
}

/// The same with the quit as the second stop: the check stops and the quit ends inside its
/// bound.
#[test]
fn the_quit_after_a_cancel_stops_the_check() {
    let mut fx = Fixture::new("graceful-cancel-then-quit", "never");
    fx.set_env("FAKE_STATUS_PAUSE_MS", "800");
    fx.listen_for_cancel();
    let (agents, _events) = login_listening(&fx);
    agents.cancel_sign_in(&codex()).unwrap();
    eventually("the check's status runs", || {
        !fx.codex_runs(&["login", "status"]).is_empty()
    });
    assert!(agents.shutdown(QUIT_WAIT), "a job still runs");
    assert_eq!(fx.codex_runs(&["logout"]), [], "the check went on");
    assert_eq!(fx.codex_runs(&["app-server"]), []);
}
