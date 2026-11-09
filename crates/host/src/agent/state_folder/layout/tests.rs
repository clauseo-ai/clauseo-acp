//! The start-up move of Claude's state folders and the notes out of `agent-state` (R17 PR-D1).

use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentKind, ErrorCode};

use super::{Adopted, Migrated, adopt, migrate, notes_at};
use crate::agent::state_folder::state_dir;
use crate::agent::state_folder::tests::{data, matter};
use crate::write;

thread_local! {
    /// The seam `moves::rename` reads: how many renames on this thread still succeed, and the
    /// kind every rename after them fails with.
    static FAIL_RENAMES: Cell<Option<(usize, io::ErrorKind)>> = const { Cell::new(None) };
}

/// The kind this rename fails with, when the seam is set and its successes are used up.
pub(super) fn next_rename_fails() -> Option<io::ErrorKind> {
    FAIL_RENAMES.with(|fail| match fail.get()? {
        (0, kind) => Some(kind),
        (left, kind) => {
            fail.set(Some((left - 1, kind)));
            None
        }
    })
}

/// Makes every move's rename on this thread fail with `kind` until the guard drops: a move
/// across volumes (`CrossesDevices`, `EXDEV` or `ERROR_NOT_SAME_DEVICE`).
pub(crate) fn fail_renames_with(kind: io::ErrorKind) -> impl Drop {
    fail_renames_after(0, kind)
}

/// As [`fail_renames_with`], after `renames` renames succeed: a start cut short mid-way.
pub(crate) fn fail_renames_after(renames: usize, kind: io::ErrorKind) -> impl Drop {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            FAIL_RENAMES.with(|fail| fail.set(None));
        }
    }
    FAIL_RENAMES.with(|fail| fail.set(Some((renames, kind))));
    Reset
}

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
            ..Migrated::default()
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

/// Where a new Claude state folder exists already (an older app made the old one again), the new
/// one is used and the old one stays as it is: never merged, never removed.
#[test]
fn an_old_claude_folder_beside_a_new_one_stays() {
    let (data, _home) = data("layout_both");
    plant_old(&data, 1);
    let claude = state_dir(&data, matter(1), "claude-acp");
    write::create_dir_all(&claude).unwrap();
    write::write_file(&claude.join(".claude.json"), b"{}").unwrap();
    let counts = migrate(&data);
    assert_eq!((counts.moved, counts.left), (1, 1), "{counts:?}");
    assert!(!claude.join("projects").exists(), "never merged");
    let transcript = old(&data, 1, "claude-acp/projects/-m/s1.jsonl");
    assert_eq!(std::fs::read(transcript).unwrap(), b"transcript");
    assert!(moved_notes(&data, 1));
}

/// Whether matter `n`'s old notes are gone and their note is in the new folder.
fn moved_notes(data: &Path, n: u32) -> bool {
    let notes = notes_at(data, &matter(n).to_string());
    std::fs::read(notes.join("n.md")).is_ok_and(|bytes| bytes == b"note")
        && !old(data, n, "notes").exists()
}

/// Old notes beside new ones (a downgrade, then an upgrade): at the start each old entry whose
/// name the new folder lacks moves in, a file and a folder whole; a clash stays in the old
/// folder, which therefore stays, and the new file is never overwritten; a link stays too. Only
/// the start merges: a launch's move leaves both.
#[test]
fn old_notes_beside_new_ones_are_merged_but_a_clash() {
    let (data, _home) = data("layout_merge");
    plant_old(&data, 1);
    let old_notes = old(&data, 1, "notes");
    write::create_dir_all(&old_notes.join("drafts")).unwrap();
    write::write_file(&old_notes.join("drafts/d.md"), b"draft").unwrap();
    write::write_file(&old_notes.join("clash.md"), b"old").unwrap();
    let notes = notes_at(&data, &matter(1).to_string());
    write::create_dir_all(&notes).unwrap();
    write::write_file(&notes.join("clash.md"), b"new").unwrap();
    assert_eq!(
        adopt(&data, &notes).unwrap(),
        Adopted::Left,
        "a launch never merges"
    );

    let counts = migrate(&data);
    assert_eq!((counts.merged, counts.clashes), (2, 1), "{counts:?}");
    assert_eq!(std::fs::read(notes.join("n.md")).unwrap(), b"note");
    assert_eq!(std::fs::read(notes.join("drafts/d.md")).unwrap(), b"draft");
    assert_eq!(std::fs::read(notes.join("clash.md")).unwrap(), b"new");
    assert_eq!(std::fs::read(old_notes.join("clash.md")).unwrap(), b"old");
    assert_eq!(
        migrate(&data).merged,
        0,
        "a second start merges nothing more"
    );

    // Without the clash the old folder empties and goes; a second start finds nothing.
    write::remove_file(&old_notes.join("clash.md")).unwrap();
    let counts = migrate(&data);
    assert_eq!((counts.merged, counts.clashes), (0, 0), "{counts:?}");
    assert!(!old_notes.exists(), "the emptied old notes are removed");
    assert_eq!(migrate(&data), Migrated::default());
}

/// A link among the old notes is never moved or followed by the merge.
#[cfg(unix)]
#[test]
fn a_link_in_old_notes_stays_in_the_merge() {
    let (data, _home) = data("layout_merge_link");
    plant_old(&data, 1);
    let old_notes = old(&data, 1, "notes");
    write::symlink(&data.join("agents"), &old_notes.join("linked")).unwrap();
    let notes = notes_at(&data, &matter(1).to_string());
    write::create_dir_all(&notes).unwrap();
    let counts = migrate(&data);
    assert_eq!((counts.merged, counts.clashes), (1, 1), "{counts:?}");
    assert!(old_notes.join("linked").is_symlink());
    assert!(std::fs::symlink_metadata(notes.join("linked")).is_err());
}

/// A move across volumes refuses the launch with words that say the agent's files are on
/// another drive, naming no path; at the start it is counted as failed, and the folder stays.
#[test]
fn a_move_across_volumes_refuses_the_launch_saying_so() {
    let (data, _home) = data("layout_other_volume");
    plant_old(&data, 1);
    let claude = state_dir(&data, matter(1), "claude-acp");
    let notes = notes_at(&data, &matter(1).to_string());
    let _seam = fail_renames_with(io::ErrorKind::CrossesDevices);
    assert_eq!(migrate(&data).failed, 2);
    let error = crate::agent::state_folder::prepare(&data, &claude, AgentKind::Claude).unwrap_err();
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
    let text = error.to_string();
    assert!(text.contains("different drive"), "{text}");
    assert!(
        !text.contains(&data.display().to_string()),
        "no path: {text}"
    );
    let error = super::refusal(&adopt(&data, &notes).unwrap_err(), "the matter's notes");
    assert!(error.to_string().contains("the matter's notes"), "{error}");
    assert!(error.to_string().contains("different drive"), "{error}");
    // Any other failure keeps the plain words.
    let other = super::refusal(&io::Error::from(io::ErrorKind::PermissionDenied), "x");
    assert!(!other.to_string().contains("drive"), "{other}");
    assert!(old(&data, 1, "claude-acp/projects/-m/s1.jsonl").exists());
    assert!(!claude.exists() && !notes.exists());
}

/// A link in the old place, at a matter's folder or at the old folder itself, is never moved or
/// followed, and the launch that needs it is refused, saying so, with nothing made in its place;
/// a link at the new root refuses the move and leaves the old folder.
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
    assert_eq!(
        (counts.moved, counts.left, counts.unmovable, counts.failed),
        (0, 0, 1, 0)
    );
    let claude = state_dir(&data, matter(1), "claude-acp");
    let error = crate::agent::state_folder::prepare(&data, &claude, AgentKind::Claude).unwrap_err();
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
    assert!(error.to_string().contains("shortcut"), "{error}");
    assert!(
        !error.to_string().contains(&data.display().to_string()),
        "{error}"
    );
    assert!(write::is_real_dir(&elsewhere.join("claude-acp")));
    assert!(!claude.exists(), "no empty folder is made in its place");
    let notes = notes_at(&data, &matter(2).to_string());
    let error = super::refusal(&adopt(&data, &notes).unwrap_err(), "the matter's notes");
    assert!(error.to_string().contains("shortcut"), "{error}");
    assert!(!notes.exists(), "no empty notes are made in their place");
    // With a new folder there already, the new one is used and the link is left.
    write::create_dir_all(&notes).unwrap();
    assert_eq!(adopt(&data, &notes).unwrap(), Adopted::Left);

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

#[path = "move_tests.rs"]
mod move_tests;
