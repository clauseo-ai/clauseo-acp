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

/// The params of each `account/read` the fake's app-server got in `agent`'s home, in order.
fn account_read_params(fx: &Fixture, agent: &AgentId) -> Vec<serde_json::Value> {
    let log = fx.config.signin.homes[agent].join("fake-account-read-params");
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_forced_read_sends_refresh_token_and_a_plain_one_does_not() {
    let fx = Fixture::new("account-codex-refresh", "ok-after:0");
    let agents = signed_in(&fx, &codex(), Method::Browser);
    let before = account_read_params(&fx, &codex()).len();
    assert_eq!(
        block_on(agents.read_account(&codex())),
        account("test@example.com")
    );
    let marker = std::fs::read(fake_marker(&fx, &codex())).unwrap();
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt as _;
        std::fs::metadata(fake_marker(&fx, &codex())).unwrap().ino()
    };
    assert_eq!(
        block_on(agents.read_account_refreshed(&codex())),
        account("test@example.com")
    );
    let params = account_read_params(&fx, &codex());
    assert_eq!(
        params[before..],
        [
            serde_json::json!({}),
            serde_json::json!({"refreshToken": true})
        ],
        "the plain read sends no refreshToken, the forced one sends true, in the same request"
    );
    // The fake's refresh rewrote the login in place, as Codex's does: the same file and bytes.
    assert_eq!(std::fs::read(fake_marker(&fx, &codex())).unwrap(), marker);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let now = std::fs::metadata(fake_marker(&fx, &codex())).unwrap().ino();
        assert_eq!(now, inode);
    }
    // Claude has no forced refresh: its forced read is its plain one.
    assert_eq!(
        block_on(agents.read_account_refreshed(&claude())),
        AccountRead::Failed,
        "Claude is not signed in on this fixture's handle"
    );
}

#[test]
fn a_forced_reads_rpc_error_is_taken_as_offline_and_no_answer_is_failed() {
    let fx = Fixture::new("account-codex-refresh-refused", "ok-after:0");
    let agents = signed_in(&fx, &codex(), Method::Browser);
    swap_to(&fx, &codex(), "refresh-refused", "test@example.com");
    assert_eq!(
        block_on(agents.read_account_refreshed(&codex())),
        AccountRead::Failed,
        "a JSON-RPC error to a forced read is taken as offline until stage B tells them apart"
    );
    assert_eq!(
        block_on(agents.read_account(&codex())),
        account("test@example.com"),
        "the plain read is untouched"
    );
    swap_to(&fx, &codex(), "refresh-silent", "test@example.com");
    assert_eq!(
        block_on(agents.read_account_refreshed(&codex())),
        AccountRead::Failed,
        "no answer tells nothing"
    );
    assert_eq!(
        block_on(agents.read_account(&codex())),
        account("test@example.com")
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

/// R17: a forced read the identity check gives up at its bound (a stalled network hangs Codex's
/// app-server past 150 s) has its app-server's stdin closed when its future is dropped, and is
/// not killed (Codex may have rotated the refresh token and not yet saved it): it drains, exits
/// on its own and is reaped.
#[cfg(target_os = "linux")]
#[test]
fn a_forced_read_let_go_closes_its_stdin_and_is_not_killed() {
    let fx = Fixture::new("account-forced-let-go", "ok-after:0");
    let agents = signed_in(&fx, &codex(), Method::Browser);
    swap_to(&fx, &codex(), "refresh-hang", "test@example.com");
    let before = account_read_params(&fx, &codex()).len();
    let read = agents.read_account_refreshed(&codex());
    crate::support::eventually("the forced request reached the app-server", || {
        account_read_params(&fx, &codex()).len() > before
    });
    let hung = fx.codex_runs(&["app-server"]).last().unwrap().pid;
    // The check's bound passes: it drops the read.
    drop(read);
    let drained = fx.config.signin.homes[&codex()].join("fake-app-server-drained");
    crate::support::eventually("the app-server read its stdin's end and exited", || {
        drained.exists()
    });
    assert!(crate::support::gone(hung), "pid {hung} still runs");
    assert_eq!(
        block_on(agents.read_account(&codex())),
        account("test@example.com"),
        "the plain read is untouched"
    );
}

/// Whether `pid` runs now (not a zombie), at once.
#[cfg(target_os = "linux")]
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        let state = stat.rsplit(')').next().unwrap_or_default().trim_start();
        !state.starts_with(['Z', 'X'])
    })
}

/// R17: while a given-up forced read's app-server still drains, another forced read of that
/// home starts none (no answer, which the check backs off on): two quick retries never leave two
/// app-servers alive. A quit kills the one draining.
#[cfg(target_os = "linux")]
#[test]
fn a_forced_read_while_one_drains_starts_no_second_app_server_and_quit_kills_it() {
    let fx = Fixture::new("account-forced-one-drains", "ok-after:0");
    let agents = signed_in(&fx, &codex(), Method::Browser);
    // The app-server is a sleep that never answers and ignores its stdin's end.
    fx.stall(Some("app-server"));
    let before = fx.codex_runs(&["app-server"]).len();
    let read = agents.read_account_refreshed(&codex());
    crate::support::eventually("the forced read's app-server runs", || {
        fx.codex_runs(&["app-server"]).len() > before
    });
    let draining = fx.codex_runs(&["app-server"])[before].pid;
    drop(read);
    for _ in 0..2 {
        assert_eq!(
            block_on(agents.read_account_refreshed(&codex())),
            AccountRead::Failed,
            "no answer while one drains"
        );
    }
    assert_eq!(
        fx.codex_runs(&["app-server"]).len(),
        before + 1,
        "no second app-server started"
    );
    assert!(alive(draining), "the drained one is not killed");
    drop(agents);
    assert!(crate::support::gone(draining), "a quit kills it");
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
        // Refresh-on-check: the forced read is the same runs and the same one request, its
        // params carrying refreshToken (the fake's refresh is a rewrite in place, no network).
        let mut forced = Vec::new();
        for _ in 0..5 {
            let start = std::time::Instant::now();
            assert_eq!(
                block_on(agents.read_account_refreshed(&agent)),
                account("test@example.com")
            );
            forced.push(start.elapsed());
        }
        forced.sort();
        eprintln!(
            "R17 timing: {agent} read_account_refreshed on the fake, median {:?}, max {:?}",
            forced[2], forced[4]
        );
        assert!(forced[4] < std::time::Duration::from_secs(5), "{forced:?}");
    }
}
