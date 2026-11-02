//! #184: a Codex agent is refused, before it starts, in a matter folder holding a `.codex`
//! entry (a folder, a file, or reached through a linked matter folder), on start, on reopen
//! (through `session/load` and `session/resume`) and before a later prompt of the same
//! agent; Claude in such a folder runs. A folder with no `.codex` runs Codex
//! (`codex::codex_handshake_streams_in_workspace_write`). And a matter folder that overlaps
//! the agent's home is refused.

use std::sync::Arc;

use clauseo_protocol::{
    DetachReason, DomainEvent, ErrorCode, StopReason, StreamId, ThreadId, TurnId,
};

use crate::codex::{CODEX, HANDSHAKE_TEXT, Recording, codex_dir, run_turn, turn_failed};
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario};
use crate::write;

/// Imports `folder` as a matter with a thread of `agent`, runs one prompt and returns the
/// thread's events with the turn.
fn turn_in(host: &Running, folder: &std::path::Path, agent: &str) -> (ThreadId, Events, TurnId) {
    let matter = host.create_matter("m", folder);
    let thread = host.create_thread(matter, agent);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(host, &mut events, thread);
    (thread, events, turn)
}

/// Asserts `turn` failed with `matter.codex_folder` and a detail that says what to do.
fn assert_refused(events: &Events, turn: TurnId) {
    let failed = turn_failed(events, turn)
        .unwrap_or_else(|| panic!("no TurnFailed of {turn}: {}", events.dump()));
    assert_eq!(
        failed.code,
        ErrorCode::MatterCodexFolder,
        "{}",
        events.dump()
    );
    let detail = failed.detail.unwrap_or_default();
    assert!(
        detail.contains("Rename or remove") && detail.contains(".codex"),
        "{detail}"
    );
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::Failed);
}

#[test]
fn codex_in_a_folder_with_a_codex_folder_is_refused_unlaunched() {
    let dir = codex_dir("codex_refused_folder", "codex-handshake");
    let folder = dir.folder(
        "m",
        &[
            ("brief.txt", "x\n"),
            (".codex/config.toml", "model = \"x\"\n"),
        ],
    );
    let launcher = Recording::new(&dir);
    let envs = Arc::clone(&launcher.envs);
    let host = dir.start_launching(0, Arc::new(launcher), |config| config);
    let (_, events, turn) = turn_in(&host, &folder, CODEX);
    assert_refused(&events, turn);
    host.shutdown();
    assert_eq!(envs.lock().unwrap().len(), 0, "the agent was launched");
}

#[test]
fn a_codex_file_counts_too() {
    let dir = codex_dir("codex_refused_file", "codex-handshake");
    let folder = dir.folder("m", &[("brief.txt", "x\n"), (".codex", "not a folder\n")]);
    let host = dir.start(0);
    let (_, events, turn) = turn_in(&host, &folder, CODEX);
    assert_refused(&events, turn);
    host.shutdown();
}

#[test]
fn codex_reopened_after_a_codex_folder_appears_is_refused() {
    let dir = codex_dir("codex_refused_reopen", "codex-handshake");
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let host = dir.start(0);
    let (thread, events, turn) = turn_in(&host, &folder, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), HANDSHAKE_TEXT);
    host.shutdown();

    write::create_dir_all(&folder.join(".codex"));
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let second = run_turn(&host, &mut events, thread);
    assert_refused(&events, second);
    host.shutdown();
}

/// The twin through the resume path: run 1 plays `codex-resume`, which takes only
/// `session/resume`.
#[test]
fn codex_resumed_after_a_codex_folder_appears_is_refused() {
    let dir = codex_dir("codex_refused_resume", "codex-handshake");
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let host = dir.start(0);
    let (thread, events, turn) = turn_in(&host, &folder, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    host.shutdown();

    write::create_dir_all(&folder.join(".codex"));
    dir.set_agents(&[(CODEX, Scenario::Bundled("codex-resume"))]);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let second = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_refused(&events, second);
    host.shutdown();
}

/// A `.codex` that appears after the first turn, while the same agent runs, fails the next
/// prompt before it is sent: `codex-handshake` expects one prompt, so a second one reaching it
/// would be a mismatch (exit 3). The agent is stopped (detached `Closed`).
#[test]
fn a_codex_folder_appearing_mid_session_fails_the_next_prompt_unsent() {
    let dir = codex_dir("codex_refused_mid_session", "codex-handshake");
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let host = dir.start(0);
    let (thread, mut events, turn) = turn_in(&host, &folder, CODEX);
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);

    write::create_dir_all(&folder.join(".codex"));
    let second = run_turn(&host, &mut events, thread);
    assert_refused(&events, second);
    let closed = |event: &DomainEvent| matches!(event, DomainEvent::AgentSessionDetached(d) if d.reason == DetachReason::Closed);
    events.until("AgentSessionDetached { Closed }", |e| {
        e.position(closed).is_some()
    });
    events.assert_no_mismatch();
    assert_eq!(events.turn_text(second), "", "{}", events.dump());
    host.shutdown();
}

/// A matter folder that holds the agent's home (the data folder's `agents` folder) is refused
/// at launch with `matter.folder_refused`, and its words name no path.
#[test]
fn a_matter_folder_holding_the_agent_home_is_refused() {
    let dir = codex_dir("codex_refused_home", "codex-handshake");
    let agents = dir.data().join("agents");
    write::create_dir_all(&agents);
    let launcher = Recording::new(&dir);
    let envs = Arc::clone(&launcher.envs);
    let host = dir.start_launching(0, Arc::new(launcher), |config| config);
    let (_, events, turn) = turn_in(&host, &agents, CODEX);
    let failed = turn_failed(&events, turn)
        .unwrap_or_else(|| panic!("no TurnFailed of {turn}: {}", events.dump()));
    assert_eq!(
        failed.code,
        ErrorCode::MatterFolderRefused,
        "{}",
        events.dump()
    );
    let detail = failed.detail.unwrap_or_default();
    assert!(!detail.contains(&*dir.data().to_string_lossy()), "{detail}");
    host.shutdown();
    assert_eq!(envs.lock().unwrap().len(), 0, "the agent was launched");
}

#[cfg(unix)]
#[test]
fn a_linked_matter_folder_holding_codex_is_refused() {
    let dir = codex_dir("codex_refused_link", "codex-handshake");
    let real = dir.folder("real", &[("brief.txt", "x\n")]);
    write::create_dir_all(&real.join(".codex"));
    let link = dir.data().join("work").join("link");
    write::symlink(&real, &link);
    let host = dir.start(0);
    let (_, events, turn) = turn_in(&host, &link, CODEX);
    assert_refused(&events, turn);
    host.shutdown();
}

#[test]
fn claude_in_a_folder_with_codex_runs() {
    let dir = Dir::new(
        "claude_beside_codex_folder",
        &[("claude-acp", Scenario::Bundled("prompt-stream"))],
    );
    let folder = dir.folder(
        "m",
        &[("brief.txt", "x\n"), (".codex/config.toml", "x = 1\n")],
    );
    let host = dir.start(0);
    let (_, events, turn) = turn_in(&host, &folder, "claude-acp");
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_ended(turn).unwrap().stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    host.shutdown();
}
