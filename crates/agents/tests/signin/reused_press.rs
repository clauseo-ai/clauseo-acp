//! A sign-in pressed while the lawyer's login is reused: the status check answers it, and no
//! login runs over that login. After an app update the window sends "Sign in" at the install's
//! `Ready`, at the moment the backend asks the first status check, which the press supersedes:
//! it used to end `signin.failed` in a few milliseconds for Codex (the guard refuses a login
//! over the lawyer's file) and to open a browser for Claude. On the fakes, as `reuse_codex.rs`
//! and `reuse_claude.rs` run them; no login here is a real credential.

use clauseo_agents::testing::write_file;
use clauseo_agents::{LoginFrom, Method, SigninState, Source, same_file};

use crate::support::{Events, Fixture, claude, codex, now};

const EMAIL: &str = "test@example.com";

fn reused(plan: &str) -> SigninState {
    SigninState::SignedIn {
        email: Some(EMAIL.to_owned()),
        plan: Some(plan.to_owned()),
        source: Source::Reused,
        at: now(),
    }
}

/// `check_status`, `LoginFound` from `from`, `use_found_login`, then `want`.
fn reuse(
    agents: &clauseo_agents::Agents,
    events: &Events,
    agent: &clauseo_protocol::AgentId,
    from: LoginFrom,
    want: &SigninState,
) {
    agents.check_status(agent).unwrap();
    assert_eq!(
        events.state(),
        SigninState::LoginFound { from, email: None }
    );
    agents.use_found_login(agent).unwrap();
    assert_eq!(&events.state(), want);
}

#[test]
fn a_codex_sign_in_racing_the_first_status_check_of_a_reused_login_ends_signed_in() {
    let fx = Fixture::new("reused-press-codex", "ok-after:0");
    let lawyer = fx.user_home().join(".codex").join("auth.json");
    write_file(&lawyer, b"").unwrap();
    let (agents, events) = fx.start_for(codex());
    reuse(
        &agents,
        &events,
        &codex(),
        LoginFrom::Codex,
        &reused("plus"),
    );
    drop(agents);

    // The next start: the install's `Ready` asks the status check, and the window's press
    // supersedes it at once.
    let (agents, events) = fx.start_for(codex());
    agents.check_status(&codex()).unwrap();
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.state(), reused("plus"));
    assert_eq!(
        fx.codex_runs(&["login"]),
        [],
        "no login runs over the lawyer's"
    );
    let home = fx.config.signin.homes[&codex()].join("auth.json");
    assert!(same_file(&home, &lawyer).unwrap(), "the link stays");

    // A press on its own, with no check before it, ends the same.
    drop(agents);
    let (agents, events) = fx.start_for(codex());
    agents.sign_in(&codex(), Method::DeviceCode).unwrap();
    assert_eq!(events.state(), reused("plus"));
    assert_eq!(fx.codex_runs(&["login"]), []);
}

#[test]
fn a_claude_sign_in_while_the_login_is_reused_ends_signed_in_with_no_login_run() {
    let fx = Fixture::new("reused-press-claude", "ok-after:0");
    let marker = fx.user_home().join(".claude/fake-signin-marker");
    let text = format!(r#"{{"authMethod":"claude.ai","email":"{EMAIL}"}}"#);
    write_file(&marker, text.as_bytes()).unwrap();
    let credential = fx
        .user_home()
        .join(".claude")
        .join(concat!(".credentials", ".json"));
    write_file(&credential, b"{}").unwrap();
    let (agents, events) = fx.start();
    reuse(
        &agents,
        &events,
        &claude(),
        LoginFrom::ClaudeCode,
        &reused("max"),
    );
    drop(agents);

    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    assert_eq!(events.state(), reused("max"));
    assert_eq!(
        fx.runs_of("login"),
        [],
        "no browser login beside the one in use"
    );
}
