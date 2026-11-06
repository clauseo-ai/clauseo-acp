//! R17 PR-C: each matter's state folder for each agent (`agent/state_folder.rs`). Claude Code's
//! `CLAUDE_CONFIG_DIR` and Codex's `CODEX_HOME` are `<data>/agent-state/<matter>/<agent>`, made
//! at the matter's first launch and cleaned of the config surfaces before every launch; the
//! login stays in the login home. The orchestrator's conditions, each with a test here: a
//! second matter's first launch needs no sign-in (Claude reads the one store through
//! `CLAUDE_SECURESTORAGE_CONFIG_DIR`; Codex gets a hard link to the existing login), and a
//! sign-out reaches every matter (`remove_login_links` clears every matter's Codex link; Claude
//! has none). A thread from before R17 PR-C gets a new session when `session/load` fails in the
//! new folder, and a reused login whose matter link is not the login's file is a split.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{AgentId, MatterId, Seq, StopReason, StreamId};
use clauseo_settings::Locations;

use crate::codex::{CODEX, run_turn};
use crate::profile_rig::{CLAUDE, Seen, Wired, state_folder};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};

fn claude() -> AgentId {
    AgentId::new(CLAUDE).unwrap()
}

/// A Claude and a Codex agent, each answering one prompt per launch.
fn two_agents(test: &str) -> Dir {
    Dir::new(
        test,
        &[
            (CLAUDE, Scenario::Bundled("claude-prompt-stream")),
            (CODEX, Scenario::Bundled("codex-handshake")),
        ],
    )
}

/// The login home the tests' `Locations` gives `agent`.
fn login_home(dir: &Dir, agent: &str) -> PathBuf {
    Locations::under_home(dir.home()).agent_home(agent)
}

/// `name` in `seen`'s environment, as text.
fn var(seen: &Seen, name: &str) -> Option<String> {
    seen.env
        .get(&OsString::from(name))
        .map(|value| value.to_str().unwrap().to_owned())
}

/// One turn on a new `agent` thread in `matter`: a new launch.
fn turn_on(host: &Running, matter: MatterId, agent: &str) {
    let thread = host.create_thread(matter, agent);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
}

/// Whether `a` and `b` are one file (Unix: device and inode, never through a link).
#[cfg(unix)]
fn one_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    let (Ok(a), Ok(b)) = (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) else {
        return false;
    };
    !a.file_type().is_symlink() && (a.dev(), a.ino()) == (b.dev(), b.ino())
}

/// Two matters, a Claude and a Codex launch in each: every launch gets its matter's own state
/// folder, never the login home and never another matter's; Claude's login store is the one
/// login home for both (no sign-in for the second matter's first launch), or the lawyer's
/// default store (`""`) while desktop says it is reused.
#[test]
fn each_matter_gets_its_own_state_folder_and_claude_one_login_store() {
    let dir = two_agents("state_folders_claude");
    let wired = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&wired), |config| config);
    let (first, claude_thread, _) =
        matter_with_thread(&dir, &host, "a", &[("brief.txt", "x\n")], CLAUDE);
    let second = host.create_matter("b", &dir.folder("b", &[("brief.txt", "y\n")]));
    let mut events = host.events(StreamId::Thread(claude_thread));
    run_turn(&host, &mut events, claude_thread);
    turn_on(&host, first, CODEX);
    turn_on(&host, second, CLAUDE);
    turn_on(&host, second, CODEX);
    wait("reused", host.handle.set_reused_login(claude(), true)).unwrap();
    turn_on(&host, second, CLAUDE);
    host.shutdown();

    let seen = wired.seen();
    let [claude_a, codex_a, claude_b, codex_b, claude_reused] = seen.as_slice() else {
        panic!("five launches, not {}", seen.len());
    };
    let store = login_home(&dir, CLAUDE).display().to_string();
    for (seen, matter, agent, var_name) in [
        (claude_a, first, CLAUDE, "CLAUDE_CONFIG_DIR"),
        (codex_a, first, CODEX, "CODEX_HOME"),
        (claude_b, second, CLAUDE, "CLAUDE_CONFIG_DIR"),
        (codex_b, second, CODEX, "CODEX_HOME"),
    ] {
        let state = state_folder(&dir, matter, agent);
        assert_eq!(var(seen, var_name), Some(state.display().to_string()));
        assert!(std::fs::symlink_metadata(&state).unwrap().is_dir());
    }
    // The second matter's first Claude launch reads the same store as the first's: no sign-in.
    assert_eq!(
        var(claude_a, "CLAUDE_SECURESTORAGE_CONFIG_DIR"),
        Some(store.clone())
    );
    assert_eq!(
        var(claude_b, "CLAUDE_SECURESTORAGE_CONFIG_DIR"),
        Some(store)
    );
    assert_eq!(
        var(claude_reused, "CLAUDE_SECURESTORAGE_CONFIG_DIR").as_deref(),
        Some("")
    );
    // Codex's `HOME` is its state folder on macOS and Linux; Claude's is never changed.
    if cfg!(unix) {
        assert_eq!(var(codex_b, "HOME"), var(codex_b, "CODEX_HOME"));
    }
    assert_ne!(var(claude_b, "HOME"), var(claude_b, "CLAUDE_CONFIG_DIR"));
    // Claude's state folder settings hold Clauseo's keys and nothing else.
    let settings = std::fs::read(state_folder(&dir, second, CLAUDE).join("settings.json")).unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&settings).unwrap();
    assert_eq!(settings, serde_json::json!({"syncClaudeAiSkills": false}));
}

/// Codex on Clauseo's own login: each matter's first launch hard-links the login home's
/// `auth.json` into its state folder (no sign-in for the second matter), and
/// `remove_login_links` then clears every matter's link and leaves the login home's file: a
/// sign-out or account switch reaches every matter. For Claude it removes nothing.
#[cfg(unix)]
#[test]
fn a_second_matter_needs_no_codex_sign_in_and_remove_login_links_clears_every_matter() {
    use crate::codex::codex_dir;
    use crate::login_check::codex;

    let dir = codex_dir("state_folders_codex", "codex-handshake");
    let home = login_home(&dir, CODEX);
    crate::write::create_dir_all(&home);
    crate::write::write(&home.join("auth.json"), b"{\"tokens\":\"own\"}");
    let host = dir.start(0);
    let first = host.create_matter("a", &dir.folder("a", &[("brief.txt", "x\n")]));
    let second = host.create_matter("b", &dir.folder("b", &[("brief.txt", "y\n")]));
    turn_on(&host, first, CODEX);
    turn_on(&host, second, CODEX);
    let links: Vec<PathBuf> = [first, second]
        .map(|matter| state_folder(&dir, matter, CODEX).join("auth.json"))
        .into();
    for link in &links {
        assert!(
            one_file(&home.join("auth.json"), link),
            "{}",
            link.display()
        );
    }
    wait("claude", host.handle.remove_login_links(claude())).unwrap();
    assert!(
        links.iter().all(|link| link.exists()),
        "Claude removes nothing"
    );
    wait("codex", host.handle.remove_login_links(codex())).unwrap();
    for link in &links {
        assert!(
            std::fs::symlink_metadata(link).is_err(),
            "{}",
            link.display()
        );
    }
    assert!(
        home.join("auth.json").exists(),
        "the login home's file stays"
    );
    host.shutdown();
}

/// What a session planted in its state folder is gone before the matter's next launch:
/// Claude's hooks, instructions and settings keys, Codex's instructions, hooks and rules, and a
/// link among them (removed as itself); the transcripts stay.
#[test]
fn the_config_surfaces_are_cleaned_before_every_launch() {
    let dir = two_agents("state_folders_clean");
    let host = dir.start(0);
    let matter = host.create_matter("a", &dir.folder("a", &[("brief.txt", "x\n")]));
    turn_on(&host, matter, CLAUDE);
    turn_on(&host, matter, CODEX);
    let claude_state = state_folder(&dir, matter, CLAUDE);
    let codex_state = state_folder(&dir, matter, CODEX);
    let planted = [
        claude_state.join("CLAUDE.md"),
        claude_state.join("hooks/run.sh"),
        claude_state.join("skills/evil/SKILL.md"),
        claude_state.join("settings.local.json"),
        codex_state.join("AGENTS.md"),
        codex_state.join("hooks.json"),
        codex_state.join("rules/default.rules"),
    ];
    let kept = [
        claude_state.join("projects/p/s.jsonl"),
        codex_state.join("sessions/2026/r.jsonl"),
    ];
    for file in planted.iter().chain(&kept) {
        crate::write::create_dir_all(file.parent().unwrap());
        crate::write::write(file, b"planted");
    }
    crate::write::write(
        &claude_state.join("settings.json"),
        br#"{"hooks":{"PreToolUse":[]},"syncClaudeAiSkills":false}"#,
    );
    crate::write::write(&codex_state.join("config.toml"), b"notify = [\"x\"]\n");
    // The first sessions end first: a launch beside a live session does not clean.
    for agent in [claude(), AgentId::new(CODEX).unwrap()] {
        wait("stop", host.handle.stop_agents(agent.clone())).unwrap();
        wait("restart", host.handle.restart_agents(agent)).unwrap();
    }
    turn_on(&host, matter, CLAUDE);
    turn_on(&host, matter, CODEX);
    host.shutdown();
    for file in &planted {
        assert!(!file.exists(), "{} stays", file.display());
    }
    for file in &kept {
        assert!(file.exists(), "{} was removed", file.display());
    }
    let settings = std::fs::read(claude_state.join("settings.json")).unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&settings).unwrap();
    assert_eq!(settings, serde_json::json!({"syncClaudeAiSkills": false}));
    let config = std::fs::read_to_string(codex_state.join("config.toml")).unwrap();
    assert!(!config.contains("notify"), "{config}");
}

/// A thread from before R17 PR-C: its stored session is in the old shared home, so
/// `session/load` fails in the matter's new folder, and the host starts a new session.
#[test]
fn a_thread_whose_session_load_fails_gets_a_new_session() {
    let dir = Dir::new(
        "state_folders_load_fails",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "a", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    run_turn(&host, &mut events, thread);
    let head = events.head();
    host.shutdown();

    dir.set_agents(&[(FAKE, Scenario::Local("load-fails-new-session"))]);
    let host = dir.start(1);
    let mut events = host.events_after(StreamId::Thread(thread), Seq::new(head).ok());
    events.until_caught_up();
    let second = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(second).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(second), "New session.");
    host.shutdown();
}

/// While reused, a matter whose `auth.json` is not the login's file (the home's link was made
/// again, so the matter's split from it) is refused as a split, `agent.auth_required` with a
/// `LoginSplit` sent, though the hook found the home's link holding; the matter's file is never
/// re-linked.
#[cfg(unix)]
#[test]
fn a_reused_login_whose_matter_link_split_is_refused_with_no_relink() {
    use clauseo_host::LoginLink;
    use clauseo_protocol::{DomainEvent, ErrorCode};

    use crate::codex::{Recording, codex_dir};
    use crate::login_check::{Fake, codex};

    let dir = codex_dir("state_folders_split", "codex-handshake");
    let home = login_home(&dir, CODEX);
    crate::write::create_dir_all(&home);
    crate::write::write(&home.join("auth.json"), b"{\"tokens\":\"lawyer\"}");
    let hook = Fake::new(LoginLink::Shared);
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let host = dir.start_launching(0, Arc::new(recording), |config| {
        config.with_login_check(hook)
    });
    let splits = host.handle.login_splits();
    let (matter, thread, _) = matter_with_thread(&dir, &host, "a", &[("brief.txt", "x\n")], CODEX);
    // Reused first (which removes every matter's old link), then a matter file that is not
    // the login's: a split made while reused.
    wait("reused", host.handle.set_reused_login(codex(), true)).unwrap();
    let state = state_folder(&dir, matter, CODEX);
    crate::write::create_dir_all(&state);
    crate::write::write(&state.join("auth.json"), b"{\"tokens\":\"stale\"}");
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::Failed);
    let code = events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
        _ => None,
    });
    assert_eq!(code, Some(ErrorCode::AgentAuthRequired));
    assert_eq!(wait("a split", splits.recv()).unwrap().agent, codex());
    assert!(launches.lock().unwrap().is_empty(), "nothing launched");
    assert_eq!(
        std::fs::read(state.join("auth.json")).unwrap(),
        b"{\"tokens\":\"stale\"}"
    );
    assert!(!one_file(&home.join("auth.json"), &state.join("auth.json")));
    host.shutdown();
}

/// The review's D3: an own login's matter link, left when that login signed out, never turns
/// the later reused login into a split. Own login, one Codex turn (the matter's link made); the
/// home's file replaced (a sign-out, then `UseFoundLogin` linking the lawyer's file); a new
/// host: `set_reused_login(true)` removes every matter's old link first, so the next launch
/// links the reused file and runs.
#[cfg(unix)]
#[test]
fn review_an_own_login_link_left_by_sign_out_splits_the_next_reuse() {
    use clauseo_host::LoginLink;

    use crate::codex::codex_dir;
    use crate::login_check::{Fake, codex};

    let dir = codex_dir("state_folders_stale", "codex-handshake");
    let home = login_home(&dir, CODEX);
    crate::write::create_dir_all(&home);
    crate::write::write(&home.join("auth.json"), b"{\"tokens\":\"own\"}");
    let host = dir.start(0);
    let matter = host.create_matter("a", &dir.folder("a", &[("brief.txt", "x\n")]));
    turn_on(&host, matter, CODEX);
    let link = state_folder(&dir, matter, CODEX).join("auth.json");
    assert!(one_file(&home.join("auth.json"), &link));
    host.shutdown();

    crate::write::remove_file(&home.join("auth.json"));
    crate::write::write(&home.join("auth.json"), b"{\"tokens\":\"lawyer\"}");
    let hook = Fake::new(LoginLink::Shared);
    let host = dir.start_with(1, |config| config.with_login_check(hook));
    wait("reused", host.handle.set_reused_login(codex(), true)).unwrap();
    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "the old link is gone"
    );
    let splits = host.handle.login_splits();
    turn_on(&host, matter, CODEX);
    assert!(splits.try_recv().is_err(), "no split");
    assert!(one_file(&home.join("auth.json"), &link));
    host.shutdown();
}

/// A matter where a Codex session runs keeps its link when the login changes (never removed
/// under a running Codex), and another matter's goes; once that session has stopped, the
/// matter's next launch removes the stale link before it links the new login.
#[cfg(unix)]
#[test]
fn a_running_matter_keeps_its_link_until_its_next_launch() {
    use clauseo_host::LoginLink;

    use crate::codex::codex_dir;
    use crate::login_check::{Fake, codex};

    let dir = codex_dir("state_folders_running", "codex-handshake");
    let home = login_home(&dir, CODEX);
    crate::write::create_dir_all(&home);
    crate::write::write(&home.join("auth.json"), b"{\"tokens\":\"own\"}");
    let hook = Fake::new(LoginLink::Shared);
    let host = dir.start_with(0, |config| config.with_login_check(hook));
    let busy = host.create_matter("a", &dir.folder("a", &[("brief.txt", "x\n")]));
    let idle = host.create_matter("b", &dir.folder("b", &[("brief.txt", "y\n")]));
    turn_on(&host, idle, CODEX);
    wait("stop", host.handle.stop_agents(codex())).unwrap();
    wait("restart", host.handle.restart_agents(codex())).unwrap();
    // The busy matter's agent stays alive after its turn.
    turn_on(&host, busy, CODEX);
    let busy_link = state_folder(&dir, busy, CODEX).join("auth.json");
    let idle_link = state_folder(&dir, idle, CODEX).join("auth.json");
    assert!(busy_link.exists() && idle_link.exists());

    crate::write::remove_file(&home.join("auth.json"));
    crate::write::write(&home.join("auth.json"), b"{\"tokens\":\"lawyer\"}");
    wait("reused", host.handle.set_reused_login(codex(), true)).unwrap();
    assert!(busy_link.exists(), "a running matter keeps its link");
    assert!(
        std::fs::symlink_metadata(&idle_link).is_err(),
        "an idle matter's goes"
    );

    wait("stop", host.handle.stop_agents(codex())).unwrap();
    wait("restart", host.handle.restart_agents(codex())).unwrap();
    let splits = host.handle.login_splits();
    turn_on(&host, busy, CODEX);
    assert!(splits.try_recv().is_err(), "no split");
    assert!(one_file(&home.join("auth.json"), &busy_link));
    host.shutdown();
}

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
    for file in &live {
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
