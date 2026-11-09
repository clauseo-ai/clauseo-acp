//! Reusing the lawyer's Claude Code login on the fake CLI, end to end through `Agents`: done-when
//! row 1's "Reuse, on the fakes" items for Claude. The lawyer's login is the fake's marker in
//! `$HOME/.claude` (the fixture's `HOME` is `<data>/user-home`), planted by the test.

use std::ffi::OsStr;
use std::path::Path;

use clauseo_agents::testing::{remove_file, write_file};
use clauseo_agents::{Found, LoginFrom, SigninError, SigninNote, SigninState, Source};

use crate::support::{Events, Fixture, claude, now};

const EMAIL: &str = "test@example.com";

/// Plants the lawyer's own Claude Code login, made with `method`, for `email`.
fn plant_lawyer(fx: &Fixture, method: &str, email: &str) {
    let marker = fx.user_home().join(".claude/fake-signin-marker");
    let text = format!(r#"{{"authMethod":"{method}","email":"{email}"}}"#);
    write_file(&marker, text.as_bytes()).unwrap();
}

fn lawyer_signed_in(fx: &Fixture) -> bool {
    fx.user_home().join(".claude/fake-signin-marker").is_file()
}

fn found(email: &str) -> SigninState {
    SigninState::LoginFound {
        from: LoginFrom::ClaudeCode,
        email: Some(email.to_owned()),
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
    assert_eq!(events.state(), found(EMAIL));
    agents.use_found_login(&claude()).unwrap();
    assert_eq!(events.state(), reused());
}

/// Detection runs with its own config folder as `CLAUDE_CONFIG_DIR` (under
/// `<data>/agents/claude-acp/detect-scratch`, gone after the run) and
/// `CLAUDE_SECURESTORAGE_CONFIG_DIR=""` (the lawyer's default store), never the inherited values,
/// and without `CODEX_HOME`. The base environment sets all three; detection is the last run.
#[test]
fn claude_detection_runs_with_its_own_config_folder() {
    let mut fx = Fixture::new("reuse-claude-env", "ok-after:0");
    let stray = fx.data.path().join("stray").into_os_string();
    for name in [
        "CLAUDE_CONFIG_DIR",
        "CLAUDE_SECURESTORAGE_CONFIG_DIR",
        "CODEX_HOME",
    ] {
        fx.config.signin.base_env.insert(name.into(), stray.clone());
    }
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found(EMAIL));
    let names = fx.env_names();
    assert!(!names.iter().any(|got| got == "CODEX_HOME"), "{names:?}");
    let detection = fx.runs_of("status").pop().unwrap();
    let env = |name: &str| detection.env.get(OsStr::new(name)).cloned();
    let config = env("CLAUDE_CONFIG_DIR").unwrap();
    let scratch = fx.marker(&claude(), "detect-scratch");
    assert!(Path::new(&config).starts_with(&scratch), "{config:?}");
    assert!(!Path::new(&config).exists(), "the run's folder is gone");
    assert_eq!(env("CLAUDE_SECURESTORAGE_CONFIG_DIR"), Some("".into()));
    assert_eq!(env("CODEX_HOME"), None);
    assert_eq!(env("HOME"), Some(fx.user_home().into_os_string()));
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
    assert_eq!(events.state(), found(EMAIL));
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

/// "a `claude.ai` status gives `LoginFound { from: ClaudeCode, email }` with the status output's
/// email, no login run and no reuse".
#[test]
fn a_claude_ai_status_gives_login_found_with_no_login_run_and_no_reuse() {
    let fx = Fixture::new("reuse-claude-found", "ok-after:0");
    plant_lawyer(&fx, "claude.ai", "lawyer@example.com");
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found("lawyer@example.com"));
    assert_eq!(fx.runs_of("login"), []);
    assert!(!fx.marker(&claude(), "login-source").exists());
    // No run had the reused session's environment: the one with the store variable is
    // detection's, in its own config folder.
    let scratch = fx.marker(&claude(), "detect-scratch");
    assert!(fx.launches().iter().all(|run| {
        !run.env
            .contains_key(OsStr::new("CLAUDE_SECURESTORAGE_CONFIG_DIR"))
            || run
                .env
                .get(OsStr::new("CLAUDE_CONFIG_DIR"))
                .is_some_and(|config| Path::new(config).starts_with(&scratch))
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
        format!("reused\n{EMAIL}\n")
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

/// "a later status with another email gives `SignedOut { found: ReusedChanged }`, a signed-out
/// one `found: ReusedSignedOut`". Each clears the reuse record only, so the next start shows
/// `LoginFound` again.
#[test]
fn another_email_or_a_signed_out_status_stops_reuse() {
    let fx = Fixture::new("reuse-claude-stops", "ok-after:0");
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    reuse(&agents, &events);
    plant_lawyer(&fx, "claude.ai", "other@example.com");
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::ReusedChanged)));
    assert!(!fx.marker(&claude(), "login-source").exists());
    assert!(!fx.marker(&claude(), "no-reuse").exists());
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found("other@example.com"));

    plant_lawyer(&fx, "claude.ai", EMAIL);
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found(EMAIL));
    agents.use_found_login(&claude()).unwrap();
    assert_eq!(events.state(), reused());
    remove_file(&fx.user_home().join(".claude/fake-signin-marker")).unwrap();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::ReusedSignedOut)));
    assert!(!fx.marker(&claude(), "login-source").exists());
    assert_eq!(fx.runs_of("logout"), []);
}

/// "`use_different_account` from `LoginFound` writes `no-reuse` and starts the sign-in run …
/// leaving the fake CLI's own status signed in".
#[test]
fn use_different_account_writes_no_reuse_and_signs_in_with_clauseo_s_own() {
    let fx = Fixture::new("reuse-claude-different", "ok-after:0");
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found(EMAIL));
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
    assert_eq!(events.state(), found(EMAIL));
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

/// "a settings-made Claude login gives `SignedOut { found: ClaudeSettings }`".
#[test]
fn a_settings_made_claude_login_gives_claude_settings() {
    for method in ["api_key_helper", "third_party", "oauth_token", "api_key"] {
        let fx = Fixture::new("reuse-claude-settings", "ok-after:0");
        plant_lawyer(&fx, method, EMAIL);
        let (agents, events) = fx.start();
        agents.check_status(&claude()).unwrap();
        assert_eq!(
            events.state(),
            signed_out(Some(Found::ClaudeSettings)),
            "{method}"
        );
        assert_eq!(
            agents.use_found_login(&claude()),
            Err(SigninError::NothingFound(claude()))
        );
    }
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

/// A Claude Code that ignores `CLAUDE_SECURESTORAGE_CONFIG_DIR` (`FAKE_SIGNIN=secure-ignored`,
/// as one that dropped the variable would) reads its store from detection's own config folder
/// too, so detection finds nothing: nothing is offered, and a login reused before the change
/// ends `ReusedSignedOut` at a later start. The run-time fallback ("status under the session
/// env says signed out while detection's status says signed in") is then never reached this
/// way; the lawyer's own login is never touched either way.
#[test]
fn a_claude_that_ignores_the_store_variable_finds_nothing_and_reuses_nothing() {
    let fx = Fixture::new("reuse-claude-fallback", "secure-ignored");
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::NoLogin)));
    assert_eq!(
        agents.use_found_login(&claude()),
        Err(SigninError::NothingFound(claude()))
    );
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
    assert_eq!(events.state(), signed_out(Some(Found::ReusedSignedOut)));
    assert!(!events.notes().contains(&SigninNote::ClaudeReuseFallback));
    assert_eq!(fx.runs_of("logout"), []);
    assert!(lawyer_signed_in(&fx));
}

/// A cancel during "Use it"'s detection (Claude's status run, stalled) ends the job
/// `signin.cancelled`, not `signin.failed`, and nothing is reused.
#[cfg(unix)]
#[test]
fn cancelling_use_it_during_its_detection_gives_cancelled() {
    let fx = Fixture::new("reuse-claude-cancel-detect", "ok-after:0");
    plant_lawyer(&fx, "claude.ai", EMAIL);
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), found(EMAIL));
    let before = fx.runs_of("status").len();
    fx.stall(Some("status"));
    agents.use_found_login(&claude()).unwrap();
    crate::support::eventually("Use it's detection started", || {
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
