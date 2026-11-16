//! The launch pass after the launch check (S15 `PR3d`): it keeps the last record when nothing
//! changed, re-probes only the folders the journal named, falls back to the full pass when it
//! can't, and removes the clean-close record either way.

use std::path::Path;

use super::super::fast::{Touched, before_launch_fast};
use super::{exit, exit_as, key, launch, launch_as, none, record, volume, world};
use crate::windows::clean_close::fake_record_path;
use crate::windows::test_world::matter_one;

/// The fast pass for session `s2`.
fn fast(root: &Path, data: &Path, touched: &[Touched]) -> Option<Vec<std::ffi::OsString>> {
    let checked = before_launch_fast(&volume(), root, data, &matter_one(), "s2", touched).unwrap();
    checked.map(|c| {
        assert_eq!(c.moved, Vec::new());
        c.unscanned
            .iter()
            .map(|p| p.as_os_str().to_owned())
            .collect()
    })
}

/// A matter launched and closed clean once, with `.git` at its root and `sub/.git` below.
fn closed(
    test: &str,
) -> (
    crate::windows::test_world::World,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let (w, root, data) = world(test);
    w.dir.file("matter/.git/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/sub/.git/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/other/brief.md", "# the lawyer's\n");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    assert_eq!(
        exit_as(&volume(), &root, &data, "s1", exit()).unwrap(),
        none()
    );
    (w, root, data)
}

#[test]
fn nothing_changed_keeps_the_last_record_and_adds_the_session_without_a_scan() {
    let (w, root, data) = closed("fast-skip");
    let before = record(&data);
    // Made where no record of the journal would say: the fast pass mustn't look.
    w.dir.file("matter/other/.git/HEAD", "x\n");
    assert_eq!(fast(&root, &data, &[]), Some(Vec::new()));
    let after = record(&data);
    assert_eq!(after.names, before.names);
    assert_eq!(after.links, before.links);
    assert_eq!(after.sessions.into_iter().collect::<Vec<_>>(), ["s2"]);
}

#[test]
fn a_touched_folder_is_probed_again_and_the_rest_kept() {
    let (w, root, data) = closed("fast-targeted");
    // The lawyer adds a `.git` in `other` between sessions; the journal names `other`.
    w.dir.file("matter/other/.git/HEAD", "x\n");
    let touched: Vec<Touched> = vec![(key("other"), false)];
    assert_eq!(fast(&root, &data, &touched), Some(Vec::new()));
    let names = record(&data).names;
    assert!(names.contains_key(&key("other/.git")));
    assert!(names.contains_key(&key("sub/.git")));
    assert!(names.contains_key(&key(".git")));
    // So the session's end doesn't take the lawyer's own `.git` for the agent's.
    assert_eq!(
        exit_as(&volume(), &root, &data, "s2", exit()).unwrap(),
        none()
    );
}

#[test]
fn a_touched_subtree_is_scanned_again_and_a_gone_folder_leaves_the_record() {
    let (w, root, data) = closed("fast-subtree");
    crate::windows::write::rename_new(&root.join("sub"), &w.at("gone-sub")).unwrap();
    w.dir.file("matter/moved/deep/.git/HEAD", "x\n");
    let touched: Vec<Touched> = vec![(key("sub"), true), (key("moved"), true)];
    assert_eq!(fast(&root, &data, &touched), Some(Vec::new()));
    let names = record(&data).names;
    assert!(!names.contains_key(&key("sub/.git")));
    assert!(names.contains_key(&key("moved/deep/.git")));
    assert_eq!(
        exit_as(&volume(), &root, &data, "s2", exit()).unwrap(),
        none()
    );
}

#[test]
fn an_open_session_or_no_record_falls_back_to_the_full_pass() {
    let (_w, root, data) = world("fast-no-record");
    assert_eq!(fast(&root, &data, &[]), None);
    let (_w, root, data) = closed("fast-open");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    assert_eq!(fast(&root, &data, &[]), None);
}

#[test]
fn both_launch_passes_remove_the_clean_close_record() {
    let (_w, root, data) = closed("fast-removes");
    let made = || {
        let path = fake_record_path(&data, &matter_one());
        let folder = path.parent().unwrap();
        crate::windows::write::make_sandbox_dir(folder).unwrap();
        crate::windows::write::write_atomic(&path, b"a record\n").unwrap();
        path
    };
    let path = made();
    assert!(path.is_file());
    fast(&root, &data, &[]).unwrap();
    assert!(!path.exists());
    let path = made();
    launch_as(&volume(), &root, &data, "s3", &["s2"], launch()).unwrap();
    assert!(!path.exists());
}

/// Round 1: after the lawyer renames `sub`, a re-probe of the root drops what the record held
/// under `sub`, so an agent that moves that `.git` back to `sub/.git` has it set aside, as
/// the full pass would.
#[test]
fn a_reprobe_drops_what_was_under_a_folder_no_longer_there_so_a_move_back_is_caught() {
    let (w, root, data) = closed("fast-renamed");
    crate::windows::write::rename_new(&root.join("sub"), &root.join("renamed")).unwrap();
    let touched: Vec<Touched> = vec![(Vec::new(), false), (key("renamed"), true)];
    assert_eq!(fast(&root, &data, &touched), Some(Vec::new()));
    let names = record(&data).names;
    assert!(!names.contains_key(&key("sub/.git")), "{names:?}");
    assert!(names.contains_key(&key("renamed/.git")));
    // The agent's session: it moves the lawyer's `.git` back under its old name.
    w.dir.dir("matter/sub");
    crate::windows::write::rename_new(&root.join("renamed/.git"), &root.join("sub/.git")).unwrap();
    let moved = exit_as(&volume(), &root, &data, "s2", exit()).unwrap();
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0].from, root.join("sub/.git"));
}

/// S15 `PR4a`: a nested `CLAUDE.md` the lawyer adds while the app is closed is recorded by the
/// re-probe of the folder the journal names, so the session's end keeps it; one the agent
/// makes in another folder is still set aside.
#[test]
fn a_reprobe_records_a_nested_claude_md_added_between_sessions() {
    let (w, root, data) = closed("fast-nested-claude");
    w.dir.file("matter/other/CLAUDE.md", "# the lawyer's\n");
    let touched: Vec<Touched> = vec![(key("other"), false)];
    assert_eq!(fast(&root, &data, &touched), Some(Vec::new()));
    assert!(record(&data).names.contains_key(&key("other/CLAUDE.md")));
    w.dir.file("matter/sub/AGENTS.md", "# the agent's\n");
    let moved = exit_as(&volume(), &root, &data, "s2", exit()).unwrap();
    let from: Vec<_> = moved.iter().map(|e| e.from.clone()).collect();
    assert_eq!(from, [root.join("sub/AGENTS.md")]);
    assert!(root.join("other/CLAUDE.md").is_file());
}
