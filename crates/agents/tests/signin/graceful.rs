//! Quit and Cancel stop Codex's sign-in runs gracefully (`signin/stopping.rs`): the browser
//! login gets `GET /cancel` on its callback port and ends writing nothing; a login in the middle
//! of saving Codex's login file finishes the save, so the file is whole; a status is awaited; a
//! run still going at the quit's bound is killed; and a 0-byte login file in Clauseo's home is
//! removed before a status reads it. No test fires the lawyer's 2 s grace: every run ends by
//! itself.

use std::net::{Ipv4Addr, TcpStream};
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

/// Whether something accepts connections on `port`. A connection the fake login reads as an
/// empty request, answers 404, and serves on.
fn listening(port: u16) -> bool {
    TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok()
}

/// Starts Codex's browser login on the fake (`script`) with its callback on a port of the
/// test's own, and waits until it listens there.
fn login_listening(fx: &Fixture, port: u16) -> (clauseo_agents::Agents, crate::support::Events) {
    let (agents, events) = start(fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    eventually("the login listens", || listening(port));
    (agents, events)
}

/// The lawyer's Cancel: `/cancel` reaches the login, which exits writing nothing; Codex's status
/// then says signed out, so the state is `Failed { Cancelled }` (the signed-out card), never the
/// `SignedIn` from before.
#[test]
fn cancel_sends_cancel_and_the_login_ends_writing_nothing() {
    let mut fx = Fixture::new("graceful-cancel", "never");
    let port = fx.listen_for_cancel();
    let (agents, events) = login_listening(&fx, port);
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
    let port = fx.listen_for_cancel();
    let (agents, _events) = login_listening(&fx, port);
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
/// second apart): the `/cancel` waits behind the save, the save finishes, the file is whole.
#[test]
fn a_quit_in_the_middle_of_a_save_leaves_the_file_whole() {
    let mut fx = Fixture::new("graceful-mid-write", "slow-save:1000");
    let port = fx.listen_for_cancel();
    let (agents, _events) = login_listening(&fx, port);
    eventually("the save started", || login_file(&fx).exists());
    assert!(agents.shutdown(QUIT_WAIT), "a job still runs");
    let text = std::fs::read_to_string(login_file(&fx)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).expect("a whole file");
    assert_eq!(saved["email"], "test@example.com");
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
