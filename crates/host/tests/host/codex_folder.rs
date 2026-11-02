//! #184: a Codex agent is refused, before it starts, in a matter folder holding a `.codex`
//! entry (a folder, a file, or reached through a linked matter folder), on start and on
//! reopen; Claude in such a folder runs. A folder with no `.codex` runs Codex
//! (`codex::codex_handshake_streams_in_workspace_write`). And `ListAgents`, and Codex's plans.

use std::sync::Arc;

use clauseo_protocol::{
    AgentKind, DomainEvent, ErrorCode, HostQuery, PlanStatus, QueryResult, StopReason, StreamId,
    ThreadId, TurnId,
};

use crate::codex::{CODEX, HANDSHAKE_TEXT, Recording, codex_dir, run_turn, turn_failed};
use crate::observe::Events;
use crate::support::{Dir, FAKE, Running, Scenario};
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

/// Asserts `turn` failed with `matter.folder_refused` and a detail that says what to do.
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

#[test]
fn list_agents_answers_each_valid_id_with_its_kind() {
    let dir = Dir::new(
        "list_agents",
        &[
            (FAKE, Scenario::Bundled("prompt-stream")),
            (CODEX, Scenario::Bundled("codex-handshake")),
            ("claude-acp", Scenario::Bundled("prompt-stream")),
            ("Bad_ID", Scenario::Bundled("prompt-stream")),
        ],
    );
    let host = dir.start(0);
    let QueryResult::Agents(agents) = host.query(HostQuery::ListAgents).unwrap() else {
        panic!("ListAgents answered another result");
    };
    let listed: Vec<(&str, Option<AgentKind>)> =
        agents.iter().map(|a| (a.id.as_str(), a.kind)).collect();
    assert_eq!(
        listed,
        [
            ("claude-acp", Some(AgentKind::Claude)),
            (CODEX, Some(AgentKind::Codex)),
            (FAKE, None),
        ]
    );
    host.shutdown();
}

/// Codex's checklist plan arrives as a standard `plan` update and shows: each update is a
/// `PlanReplaced` with every step. Its markdown proposed plan logs nothing (no protocol type).
#[test]
fn codex_plans_show() {
    let dir = codex_dir("codex_plans_show", "codex-plan");
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let (_, events, turn) = turn_in(&host, &folder, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), "Planned.");
    let plans: Vec<Vec<(String, PlanStatus)>> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PlanReplaced(plan) => Some(
                plan.entries
                    .iter()
                    .map(|entry| (entry.content.clone(), entry.status))
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    let step = |content: &str, status| (content.to_owned(), status);
    assert_eq!(
        plans,
        [
            vec![
                step("Read the lease", PlanStatus::InProgress),
                step("Draft clause 4", PlanStatus::Pending),
            ],
            vec![
                step("Read the lease", PlanStatus::Completed),
                step("Draft clause 4", PlanStatus::InProgress),
            ],
        ],
        "{}",
        events.dump()
    );
    host.shutdown();
}
