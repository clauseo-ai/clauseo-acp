//! A Claude agent's matter folder and the entries it may not hold (`agent/claude_files.rs`): a
//! `.claude/scheduled_tasks.json` or a `.git` in the matter folder, a file, a folder or a link,
//! refuses the agent before it starts, with its own code and nothing launched; one that appears
//! after the first turn fails the next prompt before it is sent.

use std::path::Path;
use std::sync::Arc;

use clauseo_protocol::{ErrorCode, StopReason, StreamId, ThreadId, TurnId};

use crate::codex::{Recording, run_turn, turn_failed};
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario};
use crate::write;

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

/// A folder whose `claude-acp` plays `claude-mode-new`, which takes one prompt.
fn claude_dir(test: &str) -> Dir {
    Dir::new(test, &[(CLAUDE, Scenario::Local("claude-mode-new"))])
}

/// Imports `folder` as a matter with a Claude thread, runs one prompt and returns the thread's
/// events with the turn.
fn turn_in(host: &Running, folder: &Path) -> (ThreadId, Events, TurnId) {
    let matter = host.create_matter("m", folder);
    let thread = host.create_thread(matter, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(host, &mut events, thread);
    (thread, events, turn)
}

/// Asserts `turn` failed with `code`, before any answer.
fn assert_refused(events: &Events, turn: TurnId, code: ErrorCode) {
    let failed = turn_failed(events, turn)
        .unwrap_or_else(|| panic!("no TurnFailed of {turn}: {}", events.dump()));
    assert_eq!(failed.code, code, "{}", events.dump());
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::Failed);
    assert_eq!(events.turn_text(turn), "", "{}", events.dump());
}

/// Each entry, as a file and as a folder, refuses the launch: nothing is launched.
#[test]
fn claude_in_a_folder_with_a_refused_entry_is_refused_unlaunched() {
    for (test, files, code) in [
        (
            "claude_refused_tasks",
            vec![(".claude/scheduled_tasks.json", "{\"tasks\":[]}\n")],
            ErrorCode::MatterClaudeTasksFile,
        ),
        (
            "claude_refused_tasks_folder",
            vec![(".claude/scheduled_tasks.json/x", "x\n")],
            ErrorCode::MatterClaudeTasksFile,
        ),
        (
            "claude_refused_git",
            vec![(".git/HEAD", "ref: refs/heads/main\n")],
            ErrorCode::MatterGitFolder,
        ),
        (
            "claude_refused_git_file",
            vec![(".git", "gitdir: /nonexistent\n")],
            ErrorCode::MatterGitFolder,
        ),
    ] {
        let dir = claude_dir(test);
        let mut all = vec![("brief.txt", "x\n")];
        all.extend(files);
        let folder = dir.folder("m", &all);
        let launcher = Recording::new(&dir);
        let envs = Arc::clone(&launcher.envs);
        let host = dir.start_launching(0, Arc::new(launcher), |config| config);
        let (_, events, turn) = turn_in(&host, &folder);
        assert_refused(&events, turn, code);
        host.shutdown();
        assert_eq!(
            envs.lock().unwrap().len(),
            0,
            "{test}: the agent was launched"
        );
    }
}

/// A link at the entry's name counts, wherever it points.
#[cfg(unix)]
#[test]
fn a_link_at_a_refused_entry_refuses_the_launch() {
    let dir = claude_dir("claude_refused_link");
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    write::create_dir_all(&folder.join(".claude"));
    write::symlink(
        Path::new("/nonexistent"),
        &folder.join(".claude/scheduled_tasks.json"),
    );
    let launcher = Recording::new(&dir);
    let envs = Arc::clone(&launcher.envs);
    let host = dir.start_launching(0, Arc::new(launcher), |config| config);
    let (_, events, turn) = turn_in(&host, &folder);
    assert_refused(&events, turn, ErrorCode::MatterClaudeTasksFile);
    host.shutdown();
    assert_eq!(envs.lock().unwrap().len(), 0, "the agent was launched");
}

/// An entry that appears after the first turn fails the next prompt before it is sent:
/// `claude-mode-new` takes one prompt, so a second reaching it would be a mismatch.
#[test]
fn a_refused_entry_appearing_mid_session_fails_the_next_prompt_unsent() {
    for (test, entry, code) in [
        (
            "claude_refused_tasks_mid",
            ".claude/scheduled_tasks.json",
            ErrorCode::MatterClaudeTasksFile,
        ),
        ("claude_refused_git_mid", ".git", ErrorCode::MatterGitFolder),
    ] {
        let dir = claude_dir(test);
        let folder = dir.folder("m", &[("brief.txt", "x\n")]);
        let host = dir.start(0);
        let (thread, mut events, turn) = turn_in(&host, &folder);
        assert_eq!(
            events.turn_ended(turn).unwrap().stop,
            StopReason::EndTurn,
            "{}",
            events.dump()
        );
        let path = folder.join(entry);
        write::create_dir_all(path.parent().unwrap());
        write::write(&path, b"{}\n");
        let second = run_turn(&host, &mut events, thread);
        assert_refused(&events, second, code);
        events.assert_no_mismatch();
        host.shutdown();
    }
}
