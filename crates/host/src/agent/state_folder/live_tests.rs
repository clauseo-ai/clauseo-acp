//! Round 3's state-folder tests: the shared login's stamp, the by-handle walk, kept files
//! with a second name, and the clean beside a live session.

use clauseo_protocol::AgentKind;

use super::tests::{data, matter, plant};
use super::{link_own, prepare, same_file, state_dir};
use crate::write;

/// Round 3's H2: an in-place write to a matter's `auth.json` is a write to the login home's
/// file (one file under two names), so the home's stamp (size, modified time) changes while its
/// file id stays: what the launch gate (#554) sees before the next launch.
#[test]
fn an_in_place_write_to_a_matters_login_changes_the_homes_stamp() {
    let (data, home) = data("stamp");
    let state = state_dir(&data, matter(12), "codex-acp");
    prepare(&data, &state, AgentKind::Codex).unwrap();
    link_own(&home, &state).unwrap();
    let source = home.join("auth.json");
    let before = std::fs::symlink_metadata(&source).unwrap();
    // In place: the same file, truncated and written (`std::fs::write` keeps the inode).
    write::write_file(
        &state.join("auth.json"),
        b"{\"tokens\":\"swapped account\"}",
    )
    .unwrap();
    let after = std::fs::symlink_metadata(&source).unwrap();
    assert!(same_file(&source, &state.join("auth.json")).unwrap());
    assert_ne!(before.len(), after.len());
    assert!(after.modified().unwrap() >= before.modified().unwrap());
    write::remove_dir_all(&data).unwrap();
}

/// Round 3's M2: a project folder swapped for a link to a folder elsewhere is removed as itself;
/// nothing outside the state folder is touched.
#[cfg(unix)]
#[test]
fn a_project_folder_swapped_for_a_link_is_removed_as_itself() {
    let (data, _home) = data("project_link");
    let state = state_dir(&data, matter(13), "claude-acp");
    prepare(&data, &state, AgentKind::Claude).unwrap();
    let outside = data.join("outside");
    write::create_dir_all(&outside.join("memory")).unwrap();
    write::write_file(&outside.join("notes.txt"), b"keep").unwrap();
    write::write_file(&outside.join("memory/MEMORY.md"), b"keep").unwrap();
    write::create_dir_all(&state.join("projects")).unwrap();
    write::symlink(&outside, &state.join("projects/p")).unwrap();
    prepare(&data, &state, AgentKind::Claude).unwrap();
    assert!(std::fs::symlink_metadata(state.join("projects/p")).is_err());
    assert!(outside.join("notes.txt").exists());
    assert!(outside.join("memory/MEMORY.md").exists());
    write::remove_dir_all(&data).unwrap();
}

/// A kept file with a second name (a hard link to a credential, say) is removed, but Codex's
/// `auth.json`, the login link; the other name is untouched.
#[test]
fn a_kept_file_with_a_second_name_is_removed_but_the_login_link() {
    let (data, home) = data("kept_links");
    let state = state_dir(&data, matter(14), "codex-acp");
    prepare(&data, &state, AgentKind::Codex).unwrap();
    link_own(&home, &state).unwrap();
    let secret = home.join("auth.json");
    write::hard_link(&secret, &state.join("history.jsonl")).unwrap();
    prepare(&data, &state, AgentKind::Codex).unwrap();
    assert!(std::fs::symlink_metadata(state.join("history.jsonl")).is_err());
    assert!(same_file(&secret, &state.join("auth.json")).unwrap());
    let claude = state_dir(&data, matter(14), "claude-acp");
    prepare(&data, &claude, AgentKind::Claude).unwrap();
    write::create_dir_all(&claude.join("projects/p")).unwrap();
    write::hard_link(&secret, &claude.join("projects/p/s.jsonl")).unwrap();
    prepare(&data, &claude, AgentKind::Claude).unwrap();
    assert!(std::fs::symlink_metadata(claude.join("projects/p/s.jsonl")).is_err());
    assert_eq!(std::fs::read(&secret).unwrap(), b"{\"tokens\":1}");
    write::remove_dir_all(&data).unwrap();
}

/// Round 3's M1: beside a live session the clean still removes every instruction surface, and
/// keeps the live session's working data (tool results, plan note, to-dos); the strict clean
/// after it removes those too.
#[test]
fn the_live_clean_removes_instructions_and_keeps_working_data() {
    let (data, _home) = data("live_clean");
    let state = state_dir(&data, matter(15), "claude-acp");
    prepare(&data, &state, AgentKind::Claude).unwrap();
    let planted = [
        ".env",
        "CLAUDE.md",
        "skills/x/SKILL.md",
        "hooks/run.sh",
        "projects/p/memory/MEMORY.md",
        ".gitconfig",
    ];
    let working = [
        "projects/p/s-1/tool-results/r.txt",
        "plans/p.md",
        "todos/t.json",
        "file-history/f",
        "shell-snapshots/s.sh",
    ];
    plant(&state, &planted);
    plant(&state, &working);
    super::prepare_in_use(&data, &state, AgentKind::Claude).unwrap();
    for name in planted {
        assert!(
            std::fs::symlink_metadata(state.join(name)).is_err(),
            "{name} stays"
        );
    }
    for name in working {
        assert!(
            state.join(name).exists(),
            "{name} was removed beside a live session"
        );
    }
    prepare(&data, &state, AgentKind::Claude).unwrap();
    for name in working {
        assert!(!state.join(name).exists(), "{name} stays the strict clean");
    }
    write::remove_dir_all(&data).unwrap();
}

/// Round 4's M2: a project folder swapped for a link to a folder elsewhere *between* the walk's
/// open and its removals (a hook in the walk does the swap): the removal goes on through the
/// folder's handle, in the folder that was opened, and nothing outside the state folder goes.
#[cfg(unix)]
#[test]
fn a_project_folder_swapped_mid_walk_never_leads_the_removal_outside() {
    let (data, _home) = data("project_swap");
    let state = state_dir(&data, matter(16), "claude-acp");
    prepare(&data, &state, AgentKind::Claude).unwrap();
    plant(
        &state,
        &["projects/p/s-1/tool-results/r.txt", "projects/p/s-1.jsonl"],
    );
    let outside = data.join("outside");
    plant(&outside, &["s-1/tool-results/r.txt", "s-1/keep.txt"]);
    let moved = state.join("projects/p-moved");
    let target = outside.clone();
    super::handle::hook::set(move |project| {
        write::rename(project, &moved).unwrap();
        write::symlink(&target, project).unwrap();
    });
    let cleaned = prepare(&data, &state, AgentKind::Claude);
    super::handle::hook::clear();
    cleaned.unwrap();
    assert!(
        outside.join("s-1/keep.txt").exists(),
        "the outside folder was touched"
    );
    assert!(outside.join("s-1/tool-results/r.txt").exists());
    // The folder that was opened was cleaned through its handle.
    assert!(!state.join("projects/p-moved/s-1").exists());
    assert!(state.join("projects/p-moved/s-1.jsonl").exists());
    write::remove_dir_all(&data).unwrap();
}

/// Round 4's low: in Codex's kept folders (`sessions`, `log`, `archived_sessions`), every file
/// with a second name and every link is removed at any depth; plain files stay.
#[cfg(unix)]
#[test]
fn codex_kept_folders_lose_hard_and_symbolic_links() {
    let (data, home) = data("kept_folder_links");
    let state = state_dir(&data, matter(17), "codex-acp");
    prepare(&data, &state, AgentKind::Codex).unwrap();
    plant(&state, &["sessions/2026/r.jsonl", "log/codex.log"]);
    let secret = home.join("auth.json");
    write::hard_link(&secret, &state.join("sessions/2026/linked.jsonl")).unwrap();
    write::symlink(&secret, &state.join("log/pointer")).unwrap();
    write::create_dir_all(&state.join("archived_sessions")).unwrap();
    write::hard_link(&secret, &state.join("archived_sessions/a.jsonl")).unwrap();
    prepare(&data, &state, AgentKind::Codex).unwrap();
    for gone in [
        "sessions/2026/linked.jsonl",
        "log/pointer",
        "archived_sessions/a.jsonl",
    ] {
        assert!(
            std::fs::symlink_metadata(state.join(gone)).is_err(),
            "{gone}"
        );
    }
    for kept in ["sessions/2026/r.jsonl", "log/codex.log"] {
        assert!(state.join(kept).exists(), "{kept}");
    }
    assert_eq!(std::fs::read(&secret).unwrap(), b"{\"tokens\":1}");
    write::remove_dir_all(&data).unwrap();
}
