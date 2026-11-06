//! R17's account read on the fake CLIs: the account each status names, hashed; another account
//! once the fake's marker names another email (the swap); no account once signed out; `Failed`
//! when status does not answer; and no sign-in state changes from a read.

use clauseo_agents::testing::{remove_file, write_file};
use clauseo_agents::{AccountHash, AccountRead, AgentsEvent, Method, SigninState};
use clauseo_protocol::AgentId;
use futures::executor::block_on;

use crate::support::{Fixture, claude, codex};

/// The fake's marker in `agent`'s home (`fake_agent`'s `fake-signin-marker`), never a credential.
fn fake_marker(fx: &Fixture, agent: &AgentId) -> std::path::PathBuf {
    fx.config.signin.homes[agent].join("fake-signin-marker")
}

/// Points the fake's marker at `email`, as another account's login would.
fn swap_to(fx: &Fixture, agent: &AgentId, method: &str, email: &str) {
    let text = format!(r#"{{"authMethod":"{method}","email":"{email}"}}"#);
    write_file(&fake_marker(fx, agent), text.as_bytes()).unwrap();
}

fn account(email: &str) -> AccountRead {
    AccountRead::Account(AccountHash::of(email).unwrap())
}

/// Signs `agent` in with `method` and waits for `SignedIn`.
fn signed_in(fx: &Fixture, agent: &AgentId, method: Method) -> clauseo_agents::Agents {
    let (agents, events) = fx.start_for(agent.clone());
    agents.sign_in(agent, method).unwrap();
    let (_, last) = events.until_end();
    assert!(matches!(last, SigninState::SignedIn { .. }), "{last:?}");
    agents
}

/// No sign-in state for `agent` among `events`: a read reports nothing.
fn no_state_for(events: &[AgentsEvent], agent: &AgentId) {
    let states = events
        .iter()
        .filter(|event| matches!(event, AgentsEvent::Signin { agent: of, .. } if of == agent));
    assert_eq!(states.count(), 0, "{events:?}");
}

#[test]
fn claude_reads_the_account_its_status_names_and_sees_a_swap() {
    let fx = Fixture::new("account-claude", "ok-after:0");
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    events.until_end();
    let before = fx.runs_of("status").len();
    assert_eq!(
        block_on(agents.read_account(&claude())),
        account("test@example.com")
    );
    assert_eq!(fx.runs_of("status").len(), before + 1, "one status run");
    swap_to(&fx, &claude(), "claude.ai", "Other@Example.com");
    assert_eq!(
        block_on(agents.read_account(&claude())),
        account("other@example.com")
    );
    remove_file(&fake_marker(&fx, &claude())).unwrap();
    assert_eq!(
        block_on(agents.read_account(&claude())),
        AccountRead::NoAccount
    );
    no_state_for(&events.drain(), &claude());
    assert!(matches!(
        agents.signin_state(&claude()),
        Some(SigninState::SignedIn { .. })
    ));
}

/// R17: a Claude status with no email but an `accountUuid` names the account by its id; one with
/// neither names none.
#[test]
fn claude_reads_an_id_only_account_and_sees_another_id() {
    let fx = Fixture::new("account-claude-id", "ok-after:0");
    let agents = signed_in(&fx, &claude(), Method::ClaudeAi);
    let marker = |text: &str| write_file(&fake_marker(&fx, &claude()), text.as_bytes()).unwrap();
    marker(r#"{"authMethod":"claude.ai","email":null,"accountUuid":"uuid-1"}"#);
    assert_eq!(block_on(agents.read_account(&claude())), account("uuid-1"));
    marker(r#"{"authMethod":"claude.ai","email":null,"accountUuid":"uuid-2"}"#);
    assert_eq!(block_on(agents.read_account(&claude())), account("uuid-2"));
    marker(r#"{"authMethod":"claude.ai","email":null}"#);
    assert_eq!(
        block_on(agents.read_account(&claude())),
        AccountRead::NoAccount
    );
}

#[test]
fn codex_reads_the_account_from_account_read_and_sees_a_swap() {
    let fx = Fixture::new("account-codex", "ok-after:0");
    let agents = signed_in(&fx, &codex(), Method::Browser);
    assert_eq!(
        block_on(agents.read_account(&codex())),
        account("test@example.com")
    );
    assert_ne!(fx.codex_runs(&["login", "status"]), []);
    swap_to(&fx, &codex(), "chatgpt", "other@example.com");
    assert_eq!(
        block_on(agents.read_account(&codex())),
        account("other@example.com")
    );
    remove_file(&fake_marker(&fx, &codex())).unwrap();
    assert_eq!(
        block_on(agents.read_account(&codex())),
        AccountRead::NoAccount
    );
}

#[test]
fn a_read_that_cannot_start_is_failed() {
    let mut fx = Fixture::new("account-not-ready", "ok-after:0");
    let _install = fx.not_installed(&codex());
    let (agents, _events) = fx.start_missing(codex());
    assert_eq!(block_on(agents.read_account(&codex())), AccountRead::Failed);
    let unknown = AgentId::new("fake-acp").unwrap();
    assert_eq!(block_on(agents.read_account(&unknown)), AccountRead::Failed);
}

#[cfg(unix)]
#[test]
fn a_status_that_never_answers_is_failed_at_its_limit() {
    let fx = Fixture::new("account-stalled", "ok-after:0");
    let agents = signed_in(&fx, &claude(), Method::ClaudeAi);
    fx.stall(Some("status"));
    let read = agents.read_account(&claude());
    fx.timer.fire(crate::support::STATUS_LIMIT);
    assert_eq!(block_on(read), AccountRead::Failed);
}

/// R17's timing: what one check costs a launch that meets a stale verdict, on the fake CLIs
/// (one process start for Claude, two for Codex: `login status`, then the app-server). The
/// numbers go to the report; the bound only catches a hang.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "measures wall time for the report; nothing under test reads the clock"
)]
fn a_read_costs_one_status_run_on_the_fakes() {
    for (agent, method) in [(claude(), Method::ClaudeAi), (codex(), Method::Browser)] {
        let fx = Fixture::new("account-timing", "ok-after:0");
        let agents = signed_in(&fx, &agent, method);
        let mut took = Vec::new();
        for _ in 0..5 {
            let start = std::time::Instant::now();
            assert_eq!(
                block_on(agents.read_account(&agent)),
                account("test@example.com")
            );
            took.push(start.elapsed());
        }
        took.sort();
        eprintln!(
            "R17 timing: {agent} read_account on the fake, median {:?}, max {:?}",
            took[2], took[4]
        );
        assert!(took[4] < std::time::Duration::from_secs(5), "{took:?}");
    }
}
