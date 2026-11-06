//! R17 PR-C's state folders beside a live session, and the launch's login rules from the review
//! rounds: no clean of a live session's working data, a busy matter at a switch to reuse, a
//! link equal to the login kept, no launch without a launch gate, a key login's missing link.
//! The helpers are `state_folders.rs`'.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{StopReason, StreamId};

use crate::codex::CODEX;
use crate::profile_rig::{CLAUDE, Wired, state_folder};
#[cfg(unix)]
use crate::state_folders::{login_home, one_file};
use crate::state_folders::{turn_on, two_agents};
#[cfg(unix)]
use crate::support::wait;

/// Round 2, defect 1: a second launch in a matter while another session of the agent still runs
/// there does not clean the shared state folder: the live session's tool results and plan note
/// stay. The config files are written all the same.
#[test]
fn review_a_second_launch_keeps_a_live_sessions_tool_results_and_plan() {
    let dir = two_agents("state_folders_live");
    let host = dir.start(0);
    let matter = host.create_matter("a", &dir.folder("a", &[("brief.txt", "x\n")]));
    // The first thread's agent stays alive after its turn.
    turn_on(&host, matter, CLAUDE);
    let state = state_folder(&dir, matter, CLAUDE);
    let live = [
        state.join("projects/p/s-1/tool-results/r.txt"),
        state.join("plans/p.md"),
    ];
    // Round 3's M1: instruction surfaces go even beside a live session.
    let planted = [
        state.join(".env"),
        state.join("CLAUDE.md"),
        state.join("skills/x/SKILL.md"),
    ];
    for file in live.iter().chain(&planted) {
        crate::write::create_dir_all(file.parent().unwrap());
        crate::write::write(file, b"live");
    }
    crate::write::write(&state.join("settings.json"), br#"{"hooks":{}}"#);
    turn_on(&host, matter, CLAUDE);
    let removed: Vec<&PathBuf> = live.iter().filter(|file| !file.exists()).collect();
    assert!(
        removed.is_empty(),
        "a live session's files were removed: {removed:?}"
    );
    for file in &planted {
        assert!(
            !file.exists(),
            "{} stays beside a live session",
            file.display()
        );
    }
    let settings = std::fs::read(state.join("settings.json")).unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&settings).unwrap();
    assert_eq!(settings, serde_json::json!({"syncClaudeAiSkills": false}));
    host.shutdown();
}

/// Round 2, defect 2: a matter busy at the switch to reuse keeps its old link, and a second
/// thread's launch there is refused with `agent.launch_failed` (retryable), not as a split:
/// reuse goes on and no split is sent. Once the busy session has ended, the next launch removes
/// the stale link and links the reused file.
#[cfg(unix)]
#[test]
fn review_a_busy_matter_at_the_switch_splits_a_second_threads_reuse() {
    use clauseo_host::LoginLink;
    use clauseo_protocol::{DomainEvent, ErrorCode};

    use crate::codex::codex_dir;
    use crate::login_check::{Fake, codex};

    let dir = codex_dir("state_folders_busy_switch", "codex-handshake");
    let home = login_home(&dir, CODEX);
    crate::write::create_dir_all(&home);
    crate::write::write(&home.join("auth.json"), b"{\"tokens\":\"own\"}");
    let hook = Fake::new(LoginLink::Shared);
    let host = dir.start_with(0, |config| config.with_login_check(hook));
    let splits = host.handle.login_splits();
    let matter = host.create_matter("a", &dir.folder("a", &[("brief.txt", "x\n")]));
    // The busy session: its agent stays alive after the turn.
    turn_on(&host, matter, CODEX);
    let link = state_folder(&dir, matter, CODEX).join("auth.json");
    crate::write::remove_file(&home.join("auth.json"));
    crate::write::write(&home.join("auth.json"), b"{\"tokens\":\"lawyer\"}");
    wait("reused", host.handle.set_reused_login(codex(), true)).unwrap();
    assert!(link.exists(), "the busy matter keeps its link");

    let second = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(second));
    let (turn, _) = host.prompt(second, "Look at the matter.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::Failed);
    let code = events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
        _ => None,
    });
    assert_eq!(code, Some(ErrorCode::AgentLaunchFailed));
    assert!(splits.try_recv().is_err(), "no split");
    assert_eq!(std::fs::read(&link).unwrap(), b"{\"tokens\":\"own\"}");

    wait("stop", host.handle.stop_agents(codex())).unwrap();
    wait("restart", host.handle.restart_agents(codex())).unwrap();
    turn_on(&host, matter, CODEX);
    assert!(splits.try_recv().is_err(), "no split");
    assert!(one_file(&home.join("auth.json"), &link));
    host.shutdown();
}

/// Round 3's M5: a login change removes only the links that are not the login home's file: a
/// link a launch made since (equal to the source) is never removed.
#[cfg(unix)]
#[test]
fn a_link_already_equal_to_the_login_survives_the_clear() {
    use clauseo_host::LoginLink;

    use crate::codex::codex_dir;
    use crate::login_check::{Fake, codex};

    let dir = codex_dir("state_folders_equal", "codex-handshake");
    let home = login_home(&dir, CODEX);
    crate::write::create_dir_all(&home);
    crate::write::write(&home.join("auth.json"), b"{\"tokens\":\"same\"}");
    let hook = Fake::new(LoginLink::Shared);
    let host = dir.start_with(0, |config| config.with_login_check(hook));
    let matter = host.create_matter("a", &dir.folder("a", &[("brief.txt", "x\n")]));
    turn_on(&host, matter, CODEX);
    wait("stop", host.handle.stop_agents(codex())).unwrap();
    wait("restart", host.handle.restart_agents(codex())).unwrap();
    let link = state_folder(&dir, matter, CODEX).join("auth.json");
    assert!(one_file(&home.join("auth.json"), &link));
    wait("reused", host.handle.set_reused_login(codex(), true)).unwrap();
    assert!(
        one_file(&home.join("auth.json"), &link),
        "an equal link stays"
    );
    host.shutdown();
}

/// Fail closed: with no launch gate wired (`HostConfig::with_launch_gate`, stage D's #554), a
/// launch of an agent with a kind, which would use the matter's state folder and the shared
/// login, is refused with `agent.launch_failed` and starts no process; the tests' pass-through
/// gate launches (every other test here).
#[test]
fn no_launch_gate_refuses_the_launch_and_starts_nothing() {
    use clauseo_protocol::{DomainEvent, ErrorCode};

    let dir = two_agents("state_folders_no_gate");
    let wired = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&wired), |config| {
        config.without_launch_gate()
    });
    let matter = host.create_matter("a", &dir.folder("a", &[("brief.txt", "x\n")]));
    for agent in [CLAUDE, CODEX] {
        let thread = host.create_thread(matter, agent);
        let mut events = host.events(StreamId::Thread(thread));
        let (turn, _) = host.prompt(thread, "Look at the matter.");
        assert_eq!(events.until_turn_ended(turn).stop, StopReason::Failed);
        let code = events.events().find_map(|event| match event {
            DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
            _ => None,
        });
        assert_eq!(code, Some(ErrorCode::AgentLaunchFailed), "{agent}");
    }
    host.shutdown();
    assert_eq!(wired.seen().len(), 0, "no agent process started");
}

/// A Codex key login (`CODEX_API_KEY` in the launch's environment) gets no `auth.json` in the
/// matter's state folder, and a link an earlier account login left there is removed: Codex can
/// use only the key, and no shared login file is in reach. Without the key the same launch links
/// the account login (`a_second_matter_needs_no_codex_sign_in_…` too).
#[cfg(unix)]
#[test]
fn a_codex_key_login_gets_no_login_link() {
    use std::ffi::OsString;

    use crate::codex::codex_dir;

    let dir = codex_dir("state_folders_key_login", "codex-handshake");
    let home = login_home(&dir, CODEX);
    crate::write::create_dir_all(&home);
    crate::write::write(&home.join("auth.json"), b"{\"tokens\":\"account\"}");
    // An account login first: the matter's link is made.
    let host = dir.start(0);
    let matter = host.create_matter("a", &dir.folder("a", &[("brief.txt", "x\n")]));
    turn_on(&host, matter, CODEX);
    let link = state_folder(&dir, matter, CODEX).join("auth.json");
    assert!(
        one_file(&home.join("auth.json"), &link),
        "an account login links"
    );
    host.shutdown();

    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| name != "CODEX_API_KEY")
        .collect();
    env.push(("CODEX_API_KEY".into(), "sk-test".into()));
    let host = dir.start_with(1, |config| config.with_inherited_env(env));
    turn_on(&host, matter, CODEX);
    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "no login link for a key login"
    );
    assert!(
        home.join("auth.json").exists(),
        "the login home's file stays"
    );
    host.shutdown();
}

/// The orchestrator's reviewer's repro of stage C's M1: one Claude turn (its agent stays
/// alive), then `CLAUDE.md` and `skills/x/SKILL.md` planted, then a second thread's launch in
/// the same matter: the planted files are gone, and the live session's `tool-results` and
/// `plans` stay.
#[test]
fn review_planted_beside_live() {
    let dir = two_agents("state_folders_planted_live");
    let host = dir.start(0);
    let matter = host.create_matter("a", &dir.folder("a", &[("brief.txt", "x\n")]));
    turn_on(&host, matter, CLAUDE);
    let state = state_folder(&dir, matter, CLAUDE);
    let live = [
        state.join("projects/p/s-1/tool-results/r.txt"),
        state.join("plans/p.md"),
    ];
    let planted = [state.join("CLAUDE.md"), state.join("skills/x/SKILL.md")];
    for file in live.iter().chain(&planted) {
        crate::write::create_dir_all(file.parent().unwrap());
        crate::write::write(file, b"x");
    }
    turn_on(&host, matter, CLAUDE);
    host.shutdown();
    for file in &planted {
        assert!(!file.exists(), "{} stays", file.display());
    }
    for file in &live {
        assert!(file.exists(), "{} was removed", file.display());
    }
}
