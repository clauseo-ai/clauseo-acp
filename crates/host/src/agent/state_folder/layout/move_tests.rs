//! The moves' own cases (`layout/moves.rs`): links inside old notes, the bounded scan, starts cut
//! short mid-way, the no-replace rename, and Windows' junctions and files held open.

use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;

use super::super::Migrated;
#[cfg(unix)]
use super::super::moves::may_hold_links;
use super::super::moves::staging;
use super::super::{adopt, migrate, notes_at, refusal};
use super::{fail_renames_after, old};
use crate::agent::state_folder::tests::{data, matter};
use crate::write;

/// Old notes of matter `n` holding `files` (relative path, text), and no other old folder.
fn plant_notes(data: &Path, n: u32, files: &[&str]) -> PathBuf {
    let notes = old(data, n, "notes");
    write::create_dir_all(&notes).unwrap();
    for file in files {
        let path = notes.join(file);
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_file(&path, file.as_bytes()).unwrap();
    }
    notes
}

/// Whether each of `files` is in `folder` with its own name as its text.
fn holds(folder: &Path, files: &[&str]) -> bool {
    files
        .iter()
        .all(|file| std::fs::read(folder.join(file)).is_ok_and(|text| text == file.as_bytes()))
}

/// A relative link inside old notes would name another place one level up, so the notes never
/// move whole: each entry holding no link moves, each link stays behind in the old folder (a
/// top-level one and one in a subfolder), nothing they point to is touched, and no staging
/// folder is left. The next start leaves the links where they are.
#[cfg(unix)]
#[test]
fn a_relative_link_in_old_notes_stays_behind_and_does_not_move() {
    let (data, _home) = data("layout_notes_links");
    let login = data.join("agents/codex-acp/home/auth.json");
    write::create_dir_all(login.parent().unwrap()).unwrap();
    write::write_file(&login, b"login").unwrap();
    let old_notes = plant_notes(&data, 1, &["n.md", "sub/deep.md", "clean/c.md"]);
    write::symlink(Path::new("../../agents"), &old_notes.join("up")).unwrap();
    write::symlink(Path::new("../../../agents"), &old_notes.join("sub/up")).unwrap();

    assert_eq!(
        migrate(&data),
        Migrated {
            moved: 1,
            ..Migrated::default()
        }
    );
    let notes = notes_at(&data, &matter(1).to_string());
    assert!(holds(&notes, &["n.md", "sub/deep.md", "clean/c.md"]));
    for link in ["up", "sub/up"] {
        assert!(old_notes.join(link).is_symlink(), "{link} stays behind");
        assert!(
            std::fs::symlink_metadata(notes.join(link)).is_err(),
            "{link} does not move"
        );
    }
    assert!(!old_notes.join("n.md").exists() && !old_notes.join("sub/deep.md").exists());
    assert!(
        !staging(&notes).unwrap().exists(),
        "no staging folder is left"
    );
    assert_eq!(std::fs::read(&login).unwrap(), b"login");

    let counts = migrate(&data);
    assert_eq!((counts.merged, counts.clashes), (0, 2), "{counts:?}");
    assert!(old_notes.join("up").is_symlink() && old_notes.join("sub/up").is_symlink());
}

/// Old notes with no link move whole, in one rename: the folder itself is the new one.
#[cfg(unix)]
#[test]
fn old_notes_with_no_link_move_whole() {
    use std::os::unix::fs::MetadataExt as _;

    let (data, _home) = data("layout_notes_whole");
    let old_notes = plant_notes(&data, 1, &["n.md", "sub/deep.md"]);
    let inode = std::fs::metadata(&old_notes).unwrap().ino();
    assert_eq!(migrate(&data).moved, 1);
    let notes = notes_at(&data, &matter(1).to_string());
    assert_eq!(std::fs::metadata(&notes).unwrap().ino(), inode);
    assert!(holds(&notes, &["n.md", "sub/deep.md"]));
}

/// The bounded scan: a folder with no link is clean; one with a link, or deeper than the cap,
/// counts as holding a link.
#[cfg(unix)]
#[test]
fn the_scan_counts_a_link_or_a_folder_past_its_cap() {
    let (data, _home) = data("layout_scan");
    let clean = data.join("clean");
    write::create_dir_all(&clean.join("a/b")).unwrap();
    write::write_file(&clean.join("a/b/f"), b"f").unwrap();
    assert!(!may_hold_links(&clean));
    let linked = data.join("linked");
    write::create_dir_all(&linked.join("a")).unwrap();
    write::symlink(Path::new("/"), &linked.join("a/root")).unwrap();
    assert!(may_hold_links(&linked));
    let deep: PathBuf = std::iter::once(data.join("deep"))
        .chain((0..40).map(|_| PathBuf::from("d")))
        .collect();
    write::create_dir_all(&deep).unwrap();
    assert!(may_hold_links(&data.join("deep")));
}

/// An entry-by-entry move cut short after one rename leaves the new name missing, the entries
/// split between the old folder and the staging folder; the next start goes on into the same
/// staging folder and then gives it the new name, the link still left behind.
#[cfg(unix)]
#[test]
fn a_staged_move_cut_short_is_finished_by_the_next_start() {
    let files = ["a.md", "b.md", "c.md"];
    let (data, _home) = data("layout_staged_cut");
    let old_notes = plant_notes(&data, 1, &files);
    write::symlink(Path::new("../.."), &old_notes.join("up")).unwrap();
    let notes = notes_at(&data, &matter(1).to_string());
    {
        let _seam = fail_renames_after(1, io::ErrorKind::Other);
        assert_eq!(migrate(&data).failed, 1);
    }
    assert!(!notes.exists(), "the new name appears only complete");
    assert!(write::is_real_dir(&staging(&notes).unwrap()));
    assert_eq!(migrate(&data).moved, 1);
    assert!(holds(&notes, &files));
    assert!(!staging(&notes).unwrap().exists());
    assert!(old_notes.join("up").is_symlink());
}

/// A staging folder a start cut short left is gone on with even when the old notes no longer
/// hold a link (a link removed in between): its entries are never left behind.
#[test]
fn a_staging_folder_left_is_never_left_behind() {
    let (data, _home) = data("layout_staged_left");
    plant_notes(&data, 1, &["a.md"]);
    let notes = notes_at(&data, &matter(1).to_string());
    let staged = super::super::moves::staging(&notes).unwrap();
    write::create_dir_all(&staged).unwrap();
    write::write_file(&staged.join("b.md"), b"b.md").unwrap();
    assert_eq!(migrate(&data).moved, 1);
    assert!(holds(&notes, &["a.md", "b.md"]));
    assert!(!staged.exists() && !old(&data, 1, "notes").exists());
}

/// Writes each of `files` (relative path) into `folder`, its own name as its text.
fn plant(folder: &Path, files: &[&str]) {
    for file in files {
        let path = folder.join(file);
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_file(&path, file.as_bytes()).unwrap();
    }
}

/// Two launches' preps racing on the same notes can leave a staging folder holding entries
/// beside the new notes folder (`moves::move_folder`). Built by hand, with no old folder left:
/// the next start drains it entry by entry, a folder the new one has too drained into it, and
/// removes it; the start after does nothing.
#[test]
fn a_staging_folder_a_race_left_beside_new_notes_is_drained_by_the_next_start() {
    let (data, _home) = data("layout_staged_race");
    let notes = notes_at(&data, &matter(1).to_string());
    let staged = staging(&notes).unwrap();
    plant(&notes, &["a.md", "sub/x.md"]);
    plant(&staged, &["b.md", "sub/y.md", "deep/z.md"]);
    assert_eq!(
        migrate(&data),
        Migrated {
            drained: 1,
            ..Migrated::default()
        }
    );
    assert!(holds(
        &notes,
        &["a.md", "sub/x.md", "b.md", "sub/y.md", "deep/z.md"]
    ));
    assert!(!staged.exists(), "the emptied staging folder goes");
    assert_eq!(migrate(&data), Migrated::default());
}

/// Draining a staging folder a race left keeps its rules: a link stays behind in it and never
/// reaches the new notes, a name the new notes have already is never replaced, and the staging
/// folder stays while it holds them; beside old notes still holding a link, too.
#[cfg(unix)]
#[test]
fn a_drained_staging_folder_keeps_its_links_and_clashes_behind() {
    let (data, _home) = data("layout_staged_race_links");
    let old_notes = plant_notes(&data, 1, &[]);
    write::symlink(Path::new("../.."), &old_notes.join("up")).unwrap();
    let notes = notes_at(&data, &matter(1).to_string());
    let staged = staging(&notes).unwrap();
    plant(&notes, &["a.md"]);
    write::create_dir_all(&staged).unwrap();
    write::write_file(&staged.join("a.md"), b"other").unwrap();
    plant(&staged, &["c.md"]);
    write::symlink(Path::new("../../agents"), &staged.join("up")).unwrap();
    let counts = migrate(&data);
    // The old notes' link (merge) and the staging folder's link and clash stay.
    assert_eq!(
        (counts.clashes, counts.drained, counts.failed),
        (3, 0, 0),
        "{counts:?}"
    );
    assert!(holds(&notes, &["a.md", "c.md"]), "a.md is never replaced");
    assert!(std::fs::symlink_metadata(notes.join("up")).is_err());
    assert!(staged.join("up").is_symlink() && staged.join("a.md").exists());
    assert!(!staged.join("c.md").exists());
}

/// A merge cut short mid-way (old notes beside new ones) keeps what moved; the next start
/// merges the rest and removes the emptied old folder.
#[test]
fn a_merge_cut_short_is_finished_by_the_next_start() {
    let files = ["a.md", "b.md", "c.md"];
    let (data, _home) = data("layout_merge_cut");
    let old_notes = plant_notes(&data, 1, &files);
    let notes = notes_at(&data, &matter(1).to_string());
    write::create_dir_all(&notes).unwrap();
    {
        let _seam = fail_renames_after(1, io::ErrorKind::Other);
        let counts = migrate(&data);
        assert_eq!((counts.merged, counts.failed), (0, 1), "{counts:?}");
    }
    let moved = files
        .iter()
        .filter(|file| notes.join(file).exists())
        .count();
    assert_eq!(moved, 1, "one entry moved before the cut");
    let counts = migrate(&data);
    assert_eq!((counts.merged, counts.failed), (2, 0), "{counts:?}");
    assert!(holds(&notes, &files));
    assert!(!old_notes.exists());
}

/// The rename never replaces anything: an empty folder or a file at the new name stays, and the
/// old one is still there.
#[test]
fn the_rename_never_replaces_a_name() {
    let (data, _home) = data("layout_no_replace");
    let from = data.join("from");
    write::create_dir_all(&from).unwrap();
    write::write_file(&from.join("f"), b"f").unwrap();
    let empty = data.join("empty");
    write::create_dir_all(&empty).unwrap();
    let file = data.join("file");
    write::write_file(&file, b"x").unwrap();
    for to in [&empty, &file] {
        let error = write::rename_no_replace(&from, to).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{error}");
        assert!(from.join("f").exists());
    }
    assert!(write::is_real_dir(&empty));
    write::rename_no_replace(&from, &data.join("free")).unwrap();
    assert!(data.join("free/f").exists() && !from.exists());
}

/// A file where old notes were refuses the launch, saying so, with no empty notes made.
#[test]
fn a_file_at_the_old_place_refuses_the_launch() {
    let (data, _home) = data("layout_old_file");
    write::create_dir_all(&old(&data, 1, "")).unwrap();
    write::write_file(&old(&data, 1, "notes"), b"not a folder").unwrap();
    assert_eq!(migrate(&data).unmovable, 1);
    let notes = notes_at(&data, &matter(1).to_string());
    let error = refusal(&adopt(&data, &notes).unwrap_err(), "the matter's notes");
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
    assert!(error.to_string().contains("is a file"), "{error}");
    assert!(!notes.exists());
}

/// Windows: an old folder that is a junction refuses the launch, saying so; the junction and
/// what it points to stay, and no empty folder is made in its place.
#[cfg(windows)]
#[test]
fn a_junction_at_the_old_place_refuses_the_launch() {
    use clauseo_protocol::AgentKind;

    use crate::agent::state_folder::state_dir;

    let (data, _home) = data("layout_junction");
    let elsewhere = data.join("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    write::write_file(&elsewhere.join("s1.jsonl"), b"transcript").unwrap();
    write::create_dir_all(&old(&data, 1, "")).unwrap();
    clauseo_sandbox::windows::test_junction::junction(&old(&data, 1, "claude-acp"), &elsewhere)
        .unwrap();
    assert_eq!(migrate(&data).unmovable, 1);
    let claude = state_dir(&data, matter(1), "claude-acp");
    let error = crate::agent::state_folder::prepare(&data, &claude, AgentKind::Claude).unwrap_err();
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
    assert!(error.to_string().contains("shortcut"), "{error}");
    assert!(!claude.exists());
    assert_eq!(
        std::fs::read(elsewhere.join("s1.jsonl")).unwrap(),
        b"transcript"
    );
}

/// Windows: a file in the old folder held open by another program stops the rename; the launch
/// is refused with words that say what to do, and the folder stays where it was.
#[cfg(windows)]
#[test]
fn a_file_held_open_refuses_the_launch_saying_what_to_do() {
    let (data, _home) = data("layout_held_open");
    let old_notes = plant_notes(&data, 1, &["n.md"]);
    let held = write::open_unshared(&old_notes.join("n.md")).unwrap();
    let notes = notes_at(&data, &matter(1).to_string());
    let error = adopt(&data, &notes).unwrap_err();
    let refused = refusal(&error, "the matter's notes");
    assert_eq!(refused.code(), ErrorCode::AgentLaunchFailed);
    assert!(
        refused
            .to_string()
            .contains("Close Clauseo's other windows or restart the computer"),
        "{refused} ({error})"
    );
    drop(held);
    assert!(holds(&old_notes, &["n.md"]));
    assert_eq!(adopt(&data, &notes).unwrap(), super::super::Adopted::Moved);
}

/// Windows: each held-handle failure reads as a file in use; any other keeps the plain words.
#[cfg(windows)]
#[test]
fn held_handles_read_as_a_file_in_use() {
    for code in [5, 32, 33] {
        let refused = refusal(&io::Error::from_raw_os_error(code), "x");
        assert!(refused.to_string().contains("in use"), "{code}: {refused}");
    }
    let other = refusal(&io::Error::from_raw_os_error(2), "x");
    assert!(!other.to_string().contains("in use"), "{other}");
}
