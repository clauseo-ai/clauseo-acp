//! A matter's `.git`: a code-tracking tool's folder, file or link in the matter folder refuses a
//! Codex agent before it starts and before a later prompt, with `matter.git_folder`, as
//! `.agents` does (`agents_folder.rs`). A `.git` only in the matter's parent refuses nothing.

use std::sync::Arc;

use clauseo_protocol::{ErrorCode, StopReason, StreamId, ThreadId, TurnId};

use crate::codex::{CODEX, HANDSHAKE_TEXT, Recording, codex_dir, run_turn, turn_failed};
use crate::observe::Events;
use crate::support::Running;
use crate::write;

/// Imports `folder` as a matter with a Codex thread, runs one prompt and returns the thread's
/// events with the turn.
fn turn_in(host: &Running, folder: &std::path::Path) -> (ThreadId, Events, TurnId) {
    let matter = host.create_matter("m", folder);
    let thread = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(host, &mut events, thread);
    (thread, events, turn)
}

/// Asserts `turn` failed with `matter.git_folder` and a detail that names the `.git` entry.
fn assert_refused(events: &Events, turn: TurnId) {
    let failed = turn_failed(events, turn)
        .unwrap_or_else(|| panic!("no TurnFailed of {turn}: {}", events.dump()));
    assert_eq!(failed.code, ErrorCode::MatterGitFolder, "{}", events.dump());
    let detail = failed.detail.unwrap_or_default();
    assert!(detail.contains("\".git\""), "{detail}");
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::Failed);
}

#[test]
fn codex_in_a_folder_with_a_git_folder_is_refused_unlaunched() {
    let dir = codex_dir("codex_refused_git_folder", "codex-handshake");
    let folder = dir.folder(
        "m",
        &[
            ("brief.txt", "x\n"),
            (".git/HEAD", "ref: refs/heads/main\n"),
        ],
    );
    let launcher = Recording::new(&dir);
    let envs = Arc::clone(&launcher.envs);
    let host = dir.start_launching(0, Arc::new(launcher), |config| config);
    let (_, events, turn) = turn_in(&host, &folder);
    assert_refused(&events, turn);
    host.shutdown();
    assert_eq!(envs.lock().unwrap().len(), 0, "the agent was launched");
}

#[test]
fn codex_in_a_folder_with_a_git_file_is_refused_unlaunched() {
    let dir = codex_dir("codex_refused_git_file", "codex-handshake");
    let folder = dir.folder(
        "m",
        &[
            ("brief.txt", "x\n"),
            (".git", "gitdir: /nonexistent/worktrees/m\n"),
        ],
    );
    let launcher = Recording::new(&dir);
    let envs = Arc::clone(&launcher.envs);
    let host = dir.start_launching(0, Arc::new(launcher), |config| config);
    let (_, events, turn) = turn_in(&host, &folder);
    assert_refused(&events, turn);
    host.shutdown();
    assert_eq!(envs.lock().unwrap().len(), 0, "the agent was launched");
}

/// A `.git` that appears after the first turn fails the next prompt before it is sent:
/// `codex-handshake` expects one prompt, so a second reaching it would be a mismatch.
#[test]
fn a_git_folder_appearing_mid_session_fails_the_next_prompt_unsent() {
    let dir = codex_dir("codex_refused_git_mid", "codex-handshake");
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let host = dir.start(0);
    let (thread, mut events, turn) = turn_in(&host, &folder);
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);

    write::create_dir_all(&folder.join(".git"));
    let second = run_turn(&host, &mut events, thread);
    assert_refused(&events, second);
    events.assert_no_mismatch();
    assert_eq!(events.turn_text(second), "", "{}", events.dump());
    host.shutdown();
}

/// A `.git` in the matter's parent is no entry of the matter: Codex launches and answers.
#[test]
fn a_git_folder_only_in_the_parent_launches() {
    let dir = codex_dir("codex_parent_git_folder", "codex-handshake");
    dir.folder("parent", &[(".git/HEAD", "ref: refs/heads/main\n")]);
    let folder = dir.folder("parent/m", &[("brief.txt", "x\n")]);
    let host = dir.start(0);
    let (_, events, turn) = turn_in(&host, &folder);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), HANDSHAKE_TEXT);
    host.shutdown();
}
