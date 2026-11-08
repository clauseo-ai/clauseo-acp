//! R1: the set-aside's own scan, and what a pass does when the scan is partial: what it found
//! is still set aside, the session stays open, and the pass names the folder it couldn't list.
//! Unix only, through `chmod`; `ntfs_tests.rs` has the NTFS case.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{exit, launch, launch_as, record, volume, world};
use crate::test_support::set_mode;
use crate::windows::set_aside::run;
use crate::windows::test_world::matter_one;

/// A folder this process can't list until the guard goes (`chmod 000`), so the scan can't list
/// it, as NTFS refuses a folder whose DACL shuts the lawyer out.
struct Unlistable(PathBuf);

impl Unlistable {
    fn new(folder: &Path) -> Unlistable {
        set_mode(folder, 0o000);
        Unlistable(folder.to_owned())
    }
}

impl Drop for Unlistable {
    fn drop(&mut self) {
        set_mode(&self.0, 0o755);
    }
}

/// R1: the agent makes `.claude` at the root and a folder the scan can't list. The pass sets
/// `.claude` aside, names the folder, and keeps the session open; once the folder can be
/// listed, the next launch checks it as a crashed session's and sets aside the `.git` in it.
#[test]
fn a_partial_scan_sets_aside_what_it_found_names_the_folder_and_keeps_the_session_open() {
    let (w, root, data) = world("aside-partial");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    w.dir.file("matter/.claude/settings.json", "{}");
    w.dir
        .file("matter/locked/.git/HEAD", "ref: refs/heads/main\n");

    let locked = Unlistable::new(&root.join("locked"));
    let checked = run::after_session(&volume(), &root, &data, &matter_one(), "s1", exit()).unwrap();

    let moved: Vec<_> = checked.moved.iter().map(|e| e.from.clone()).collect();
    assert_eq!(moved, [root.join(".claude")]);
    assert!(checked.partial());
    assert_eq!(checked.unscanned, [root.join("locked")]);
    let open: Vec<_> = record(&data).sessions.into_iter().collect();
    assert_eq!(open, ["s1"], "the session stays open");

    drop(locked);
    let later = exit() + Duration::from_secs(60);
    let moved = launch_as(&volume(), &root, &data, "s2", &[], later).unwrap();
    let from: Vec<_> = moved.iter().map(|e| e.from.clone()).collect();
    assert_eq!(from, [root.join("locked").join(".git")]);
}
