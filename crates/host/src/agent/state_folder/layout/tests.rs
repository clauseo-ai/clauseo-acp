//! The start-up move of Claude's state folders and the notes out of `agent-state` (R17 PR-D1).

use std::path::{Path, PathBuf};

use super::{Adopted, Migrated, adopt, migrate, notes_at};
use crate::agent::state_folder::state_dir;
use crate::agent::state_folder::tests::{data, matter};
use crate::write;

/// `<data>/agent-state/<matter n>/<name>`: the old place.
fn old(data: &Path, n: u32, name: &str) -> PathBuf {
    data.join("agent-state")
        .join(matter(n).to_string())
        .join(name)
}

/// The old layout for matter `n`: a Claude state folder with a transcript, a Codex one with a
/// rollout, and notes with a note.
fn plant_old(data: &Path, n: u32) {
    let claude = old(data, n, "claude-acp");
    write::create_dir_all(&claude.join("projects/-m")).unwrap();
    write::write_file(&claude.join("projects/-m/s1.jsonl"), b"transcript").unwrap();
    let codex = old(data, n, "codex-acp");
    write::create_dir_all(&codex.join("sessions")).unwrap();
    write::write_file(&codex.join("sessions/r.jsonl"), b"rollout").unwrap();
    let notes = old(data, n, "notes");
    write::create_dir_all(&notes).unwrap();
    write::write_file(&notes.join("n.md"), b"note").unwrap();
}

/// Whether matter `n` is in the new layout, everything it held kept.
fn moved(data: &Path, n: u32) -> bool {
    let claude = state_dir(data, matter(n), "claude-acp");
    let codex = state_dir(data, matter(n), "codex-acp");
    let notes = notes_at(data, &matter(n).to_string());
    let read = |path: PathBuf| std::fs::read(path).unwrap_or_default();
    read(claude.join("projects/-m/s1.jsonl")) == b"transcript"
        && read(codex.join("sessions/r.jsonl")) == b"rollout"
        && read(notes.join("n.md")) == b"note"
        && !old(data, n, "claude-acp").exists()
        && !old(data, n, "notes").exists()
}

/// The helpers name the new layout: Claude's state in `claude-state`, the notes in `notes`,
/// Codex's state where it was.
#[test]
fn the_helpers_name_the_new_layout() {
    let data = PathBuf::from("/data");
    let m = matter(1).to_string();
    assert_eq!(
        state_dir(&data, matter(1), "claude-acp"),
        data.join("claude-state").join(&m).join("claude-acp")
    );
    assert_eq!(
        state_dir(&data, matter(1), "codex-acp"),
        data.join("agent-state").join(&m).join("codex-acp")
    );
    assert_eq!(notes_at(&data, &m), data.join("notes").join(&m));
}

/// The old layout moves at the start, Codex's state stays; a second start does nothing.
#[test]
fn the_old_layout_moves_once_and_codex_stays() {
    let (data, _home) = data("layout_moves");
    plant_old(&data, 1);
    plant_old(&data, 2);
    let codex = old(&data, 1, "codex-acp");
    assert_eq!(
        migrate(&data),
        Migrated {
            moved: 4,
            left: 0,
            failed: 0
        }
    );
    assert!(moved(&data, 1) && moved(&data, 2));
    assert!(
        write::is_real_dir(&codex),
        "Codex's state folder never moves"
    );
    assert_eq!(
        migrate(&data),
        Migrated::default(),
        "a second start moves nothing"
    );
    assert!(moved(&data, 1) && moved(&data, 2));
}

/// A start cut short (one matter moved, the other's notes moved but not its Claude state) is
/// finished by the next start, and nothing already moved is touched.
#[test]
fn a_start_cut_short_is_finished_by_the_next() {
    let (data, _home) = data("layout_cut_short");
    plant_old(&data, 1);
    plant_old(&data, 2);
    let notes = notes_at(&data, &matter(2).to_string());
    assert_eq!(adopt(&data, &notes).unwrap(), Adopted::Moved);
    let claude = state_dir(&data, matter(1), "claude-acp");
    assert_eq!(adopt(&data, &claude).unwrap(), Adopted::Moved);
    assert_eq!(migrate(&data).moved, 2);
    assert!(moved(&data, 1) && moved(&data, 2));
}

/// No data folder state at all, or an empty `agent-state`, moves nothing.
#[test]
fn nothing_to_move_is_fine() {
    let (data, _home) = data("layout_nothing");
    assert_eq!(migrate(&data), Migrated::default());
    write::create_dir_all(&data.join("agent-state")).unwrap();
    assert_eq!(migrate(&data), Migrated::default());
    let notes = notes_at(&data, &matter(1).to_string());
    assert_eq!(adopt(&data, &notes).unwrap(), Adopted::Nothing);
    assert!(!notes.exists(), "nothing is made when nothing moved");
}

/// Where a new folder exists already (an older app made the old one again), the new one is used
/// and the old one stays as it is: never merged, never removed.
#[test]
fn an_old_folder_beside_a_new_one_stays() {
    let (data, _home) = data("layout_both");
    plant_old(&data, 1);
    let notes = notes_at(&data, &matter(1).to_string());
    write::create_dir_all(&notes).unwrap();
    write::write_file(&notes.join("new.md"), b"new").unwrap();
    let counts = migrate(&data);
    assert_eq!((counts.moved, counts.left), (1, 1));
    assert_eq!(std::fs::read(notes.join("new.md")).unwrap(), b"new");
    assert!(!notes.join("n.md").exists());
    assert_eq!(std::fs::read(old(&data, 1, "notes/n.md")).unwrap(), b"note");
}

/// A link in the old place, at a matter's folder or at `agent-state`, is never moved or
/// followed; a link at the new root refuses the move and leaves the old folder.
#[cfg(unix)]
#[test]
fn links_are_never_moved_or_followed() {
    let (data, _home) = data("layout_links");
    let elsewhere = data.join("elsewhere");
    write::create_dir_all(&elsewhere.join("claude-acp")).unwrap();
    write::create_dir_all(&data.join("agent-state")).unwrap();
    // A matter folder that is a link: skipped, and an adopt through it leaves it.
    write::symlink(
        &elsewhere,
        &old(&data, 1, "").components().collect::<PathBuf>(),
    )
    .unwrap();
    // An old notes folder that is a link.
    write::create_dir_all(&old(&data, 2, "")).unwrap();
    write::symlink(&elsewhere, &old(&data, 2, "notes")).unwrap();
    let counts = migrate(&data);
    assert_eq!((counts.moved, counts.left, counts.failed), (0, 1, 0));
    let claude = state_dir(&data, matter(1), "claude-acp");
    assert_eq!(adopt(&data, &claude).unwrap(), Adopted::Left);
    assert!(write::is_real_dir(&elsewhere.join("claude-acp")));
    assert!(!claude.exists());

    let (data, _home) = self::data("layout_link_root");
    plant_old(&data, 1);
    write::symlink(&elsewhere, &data.join("notes")).unwrap();
    let counts = migrate(&data);
    assert_eq!((counts.moved, counts.failed), (1, 1));
    assert!(
        old(&data, 1, "notes/n.md").exists(),
        "the notes stay where they were"
    );
    assert!(!elsewhere.join(matter(1).to_string()).exists());
}

/// A Claude launch moves the state folder a failed start left at the old place before it makes
/// the folder, and is refused while the move fails (here `claude-state` is a file): never a
/// launch on an empty new folder while the old one holds the transcripts.
#[test]
fn a_claude_launch_finishes_a_failed_move_or_is_refused() {
    use clauseo_protocol::{AgentKind, ErrorCode};

    let (data, _home) = data("layout_launch");
    plant_old(&data, 1);
    let claude = state_dir(&data, matter(1), "claude-acp");
    crate::agent::state_folder::prepare(&data, &claude, AgentKind::Claude).unwrap();
    assert_eq!(
        std::fs::read(claude.join("projects/-m/s1.jsonl")).unwrap(),
        b"transcript"
    );
    assert!(!old(&data, 1, "claude-acp").exists());

    let (data, _home) = self::data("layout_launch_refused");
    plant_old(&data, 1);
    write::write_file(&data.join("claude-state"), b"not a folder").unwrap();
    let claude = state_dir(&data, matter(1), "claude-acp");
    let error = crate::agent::state_folder::prepare(&data, &claude, AgentKind::Claude).unwrap_err();
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
    assert!(
        !error.to_string().contains(&data.display().to_string()),
        "{error}"
    );
    assert!(old(&data, 1, "claude-acp/projects/-m/s1.jsonl").exists());
}
