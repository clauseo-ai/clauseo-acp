//! Reusing the lawyer's Claude Code login on the fake CLI, end to end through `Agents`: done-when
//! row 1's "Reuse, on the fakes" items for Claude. The lawyer's login is a stand-in credential
//! file in `$HOME/.claude`, which detection stats, and the fake's marker beside it, which the
//! fake's status reads (the fixture's `HOME` is `<data>/user-home`); neither is a real
//! credential.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use clauseo_agents::testing::{remove_file, write_file};
use clauseo_agents::{Found, LoginFrom, SigninError, SigninNote, SigninState, Source};

use crate::support::{Events, Fixture, claude, now};

const EMAIL: &str = "test@example.com";

/// The lawyer's stand-in credential file, which detection stats.
fn credential(fx: &Fixture) -> PathBuf {
    fx.user_home()
        .join(".claude")
        .join(concat!(".credentials", ".json"))
}

/// Plants the lawyer's own Claude Code login, made with `method`, for `email`.
fn plant_lawyer(fx: &Fixture, method: &str, email: &str) {
    let marker = fx.user_home().join(".claude/fake-signin-marker");
    let text = format!(r#"{{"authMethod":"{method}","email":"{email}"}}"#);
    write_file(&marker, text.as_bytes()).unwrap();
    write_file(&credential(fx), b"{}").unwrap();
}

/// Signs the lawyer's own Claude Code out: the marker and the credential file go.
fn sign_lawyer_out(fx: &Fixture) {
    remove_file(&fx.user_home().join(".claude/fake-signin-marker")).unwrap();
    remove_file(&credential(fx)).unwrap();
}

fn lawyer_signed_in(fx: &Fixture) -> bool {
    fx.user_home().join(".claude/fake-signin-marker").is_file()
}

/// Detection's `LoginFound` for Claude: never an email (it is a stat of the credential file).
fn found() -> SigninState {
    SigninState::LoginFound {
        from: LoginFrom::ClaudeCode,
        email: None,
    }
}

fn reused() -> SigninState {
    SigninState::SignedIn {
        email: Some(EMAIL.to_owned()),
        plan: Some("max".to_owned()),
        source: Source::Reused,
        at: now(),
    }
}

fn signed_out(found: Option<Found>) -> SigninState {
    SigninState::SignedOut { found }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// `check_status`, then `LoginFound`, then `use_found_login`, then `SignedIn { Reused }`.
fn reuse(agents: &clauseo_agents::Agents, events: &Events) {
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found());
    agents.use_found_login(&claude()).unwrap();
    assert_eq!(events.state(), reused());
}

/// Detection starts no run: with the lawyer's login there, the status check makes one run,
/// the status in Clauseo's own home, though the base environment sets the store and Codex
/// variables (which that run never gets).
#[test]
fn claude_detection_starts_no_run() {
    let mut fx = Fixture::new("reuse-claude-env", "ok-after:0");
    let stray = fx.data.path().join("stray").into_os_string();
    for name in ["CLAUDE_SECURESTORAGE_CONFIG_DIR", "CODEX_HOME"] {
        fx.config.signin.base_env.insert(name.into(), stray.clone());
    }
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found());
    let home = fx.config.signin.homes[&claude()].clone().into_os_string();
    let runs = fx.launches();
    assert_eq!(runs.len(), 1, "{runs:?}");
    let env = |name: &str| runs[0].env.get(OsStr::new(name)).cloned();
    assert_eq!(env("CLAUDE_CONFIG_DIR"), Some(home));
    assert_eq!(env("CLAUDE_SECURESTORAGE_CONFIG_DIR"), None);
    assert_eq!(env("CODEX_HOME"), None);
}

/// No sign-in or status run gets `NODE_OPTIONS` or `NODE_PATH`, though the base environment
/// sets both: neither reaches the fake CLI's environment nor any run's spec.
#[test]
fn a_status_run_gets_neither_node_options_nor_node_path() {
    let mut fx = Fixture::new("reuse-claude-node-env", "ok-after:0");
    for name in clauseo_process::NODE_ENV {
        fx.config
            .signin
            .base_env
            .insert(name.into(), fx.data.path().join("stray").into_os_string());
    }
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found());
    let names = fx.env_names();
    assert!(names.iter().any(|got| got == "HOME"), "{names:?}");
    for name in clauseo_process::NODE_ENV {
        assert!(!names.iter().any(|got| got == name), "{name}: {names:?}");
    }
    assert!(fx.launches().iter().all(|run| {
        clauseo_process::NODE_ENV
            .iter()
            .all(|name| !run.env.contains_key(OsStr::new(name)))
    }));
}

/// The lawyer's credential file there gives `LoginFound { from: ClaudeCode, email: None }`, no
/// login run and no reuse.
#[test]
fn a_login_file_gives_login_found_with_no_login_run_and_no_reuse() {
    let fx = Fixture::new("reuse-claude-found", "ok-after:0");
    plant_lawyer(&fx, "claude.ai", "lawyer@example.com");
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found());
    assert_eq!(fx.runs_of("login"), []);
    assert!(!fx.marker(&claude(), "login-source").exists());
    // No run had the reused session's environment.
    assert!(fx.launches().iter().all(|run| {
        !run.env
            .contains_key(OsStr::new("CLAUDE_SECURESTORAGE_CONFIG_DIR"))
    }));
}

/// "`use_found_login` then gives `SignedIn { source: Reused }`, and the next start gives `Reused`
/// without asking".
#[test]
fn use_found_login_reuses_and_the_next_start_reuses_without_asking() {
    let fx = Fixture::new("reuse-claude-use", "ok-after:0");
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    reuse(&agents, &events);
    assert_eq!(
        read(&fx.marker(&claude(), "login-source")),
        "reused\n".to_owned()
    );
    // The status before the first session ran under the session's environment.
    let home = fx.config.signin.homes[&claude()].clone();
    assert!(fx.runs_of("status").iter().any(|run| {
        run.env.get(OsStr::new("CLAUDE_SECURESTORAGE_CONFIG_DIR")) == Some(&"".into())
            && run.env.get(OsStr::new("CLAUDE_CONFIG_DIR")) == Some(&home.clone().into_os_string())
    }));
    assert_eq!(fx.runs_of("login"), []);
    drop(agents);
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), reused());
    assert_eq!(fx.runs_of("login"), []);
}

/// An account change while reused is not flagged (the agent runs as the lawyer, and switching
/// accounts is their own doing): the status under the session's environment shows the new
/// account. No login file any more gives `found: ReusedSignedOut`, which clears the reuse
/// record only.
#[test]
fn a_changed_account_shows_through_and_a_signed_out_status_stops_reuse() {
    let fx = Fixture::new("reuse-claude-stops", "ok-after:0");
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    reuse(&agents, &events);
    plant_lawyer(&fx, "claude.ai", "other@example.com");
    agents.check_status(&claude()).unwrap();
    assert_eq!(
        events.state(),
        SigninState::SignedIn {
            email: Some("other@example.com".to_owned()),
            plan: Some("max".to_owned()),
            source: Source::Reused,
            at: now(),
        }
    );
    sign_lawyer_out(&fx);
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::ReusedSignedOut)));
    assert!(!fx.marker(&claude(), "login-source").exists());
    assert!(!fx.marker(&claude(), "no-reuse").exists());
    assert_eq!(fx.runs_of("logout"), []);
    plant_lawyer(&fx, "claude.ai", EMAIL);
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found());
}

/// "`use_different_account` from `LoginFound` writes `no-reuse` and starts the sign-in run …
/// leaving the fake CLI's own status signed in".
#[test]
fn use_different_account_writes_no_reuse_and_signs_in_with_clauseo_s_own() {
    let fx = Fixture::new("reuse-claude-different", "ok-after:0");
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found());
    agents.use_different_account(&claude()).unwrap();
    let (_, last) = events.until_end();
    assert!(
        matches!(
            last,
            SigninState::SignedIn {
                source: Source::Own,
                ..
            }
        ),
        "{last:?}"
    );
    assert!(fx.marker(&claude(), "no-reuse").is_file());
    assert_eq!(read(&fx.marker(&claude(), "login-source")), "own\n");
    assert_eq!(fx.runs_of("login").len(), 1);
    assert!(lawyer_signed_in(&fx));
    assert_eq!(fx.runs_of("logout"), []);
}

/// "`stop_using_login` writes `no-reuse` and runs no logout, … leaving the fake CLI's own status
/// signed in". `find_login` then deletes `no-reuse` and detects again.
#[test]
fn stop_using_login_writes_no_reuse_and_runs_no_logout() {
    let fx = Fixture::new("reuse-claude-stop", "ok-after:0");
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    reuse(&agents, &events);
    agents.stop_using_login(&claude(), None).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::ReuseOff)));
    assert!(fx.marker(&claude(), "no-reuse").is_file());
    assert!(!fx.marker(&claude(), "login-source").exists());
    assert_eq!(fx.runs_of("logout"), []);
    assert!(lawyer_signed_in(&fx));
    // The next start does not reuse again without asking, nor offer it.
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::ReuseOff)));
    agents.find_login(&claude()).unwrap();
    assert_eq!(events.state(), found());
    assert!(!fx.marker(&claude(), "no-reuse").exists());
}

/// UD2: with no login of the lawyer's, the status check and `find_login` say none was found;
/// a reason that is no stop reason stops reuse as no reason does.
#[test]
fn no_lawyer_login_gives_none_found_and_a_non_reason_stop_turns_reuse_off() {
    let fx = Fixture::new("reuse-claude-none", "ok-after:0");
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::NoLogin)));
    agents.find_login(&claude()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::NoLogin)));

    plant_lawyer(&fx, "claude.ai", EMAIL);
    reuse(&agents, &events);
    agents
        .stop_using_login(&claude(), Some(Found::NoLogin))
        .unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::ReuseOff)));
    assert!(fx.marker(&claude(), "no-reuse").is_file());
}

/// "no `auth logout` … ever runs while the source is `Reused`": the call refuses by the state,
/// and a fresh `Agents` (no state yet) is refused by the job, from `login-source`, which then
/// reports the reused login.
#[test]
fn no_claude_logout_runs_while_the_source_is_reused() {
    let fx = Fixture::new("reuse-claude-no-logout", "ok-after:0");
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    reuse(&agents, &events);
    assert_eq!(
        agents.sign_out(&claude()),
        Err(SigninError::Reused(claude()))
    );
    assert_eq!(
        agents.use_different_account(&claude()),
        Err(SigninError::Reused(claude()))
    );
    drop(agents);
    let (agents, events) = fx.start();
    agents.sign_out(&claude()).unwrap();
    assert_eq!(events.state(), reused());
    assert_eq!(fx.runs_of("logout"), []);
    assert!(lawyer_signed_in(&fx));
}

/// "the run-time fallback": "If status under the session env says signed out while detection's
/// status says signed in (the variable changed upstream), S18 logs `claude_reuse_fallback` and
/// offers Clauseo's own sign-in." Detection is a stat, so it still finds the login;
/// `FAKE_SIGNIN=secure-ignored` makes the fake drop `CLAUDE_SECURESTORAGE_CONFIG_DIR`, as such a
/// Claude Code would: on "Use it", and at a later start of a login reused before the change.
#[test]
fn the_run_time_fallback_logs_and_offers_clauseo_s_own_sign_in() {
    let fx = Fixture::new("reuse-claude-fallback", "secure-ignored");
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found());
    agents.use_found_login(&claude()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::ReuseOff)));
    assert_eq!(events.notes(), [SigninNote::ClaudeReuseFallback]);
    assert_eq!(
        SigninNote::ClaudeReuseFallback.code(),
        "claude_reuse_fallback"
    );
    assert!(!fx.marker(&claude(), "login-source").exists());
    assert_eq!(fx.runs_of("logout"), []);
    drop(agents);

    let mut fx = Fixture::new("reuse-claude-fallback-later", "ok-after:0");
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    reuse(&agents, &events);
    drop(agents);
    fx.config
        .signin
        .base_env
        .insert("FAKE_SIGNIN".into(), "secure-ignored".into());
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::ReuseOff)));
    assert_eq!(events.notes(), [SigninNote::ClaudeReuseFallback]);
    assert!(lawyer_signed_in(&fx));
}

/// A cancel during "Use it"'s status under the session's environment (stalled) ends the job
/// `signin.cancelled`, not `signin.failed`, and nothing is reused.
#[cfg(unix)]
#[test]
fn cancelling_use_it_during_its_status_gives_cancelled() {
    let fx = Fixture::new("reuse-claude-cancel-detect", "ok-after:0");
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found());
    let before = fx.runs_of("status").len();
    fx.stall(Some("status"));
    agents.use_found_login(&claude()).unwrap();
    crate::support::eventually("Use it's status started", || {
        fx.runs_of("status").len() > before
    });
    agents.cancel_sign_in(&claude()).unwrap();
    // A status is awaited on Cancel; this one never answers, so it is killed past the grace.
    fx.timer.fire(std::time::Duration::from_secs(2));
    assert_eq!(
        events.state(),
        SigninState::Failed {
            code: clauseo_agents::SigninCode::Cancelled,
            at: now()
        }
    );
    assert!(!fx.marker(&claude(), "login-source").exists());
}

/// UD2: a search whose status run never answers (killed at its 5 s limit) ends
/// `signin.failed`, so the lawyer who asked gets an answer; the state is never left as it was.
#[cfg(unix)]
#[test]
fn a_find_login_that_cannot_tell_ends_failed() {
    let fx = Fixture::new("reuse-claude-find-stalled", "ok-after:0");
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::NoLogin)));
    fx.stall(Some("status"));
    agents.find_login(&claude()).unwrap();
    fx.timer.fire(std::time::Duration::from_secs(5));
    assert_eq!(
        events.state(),
        SigninState::Failed {
            code: clauseo_agents::SigninCode::Failed,
            at: now()
        }
    );
}
