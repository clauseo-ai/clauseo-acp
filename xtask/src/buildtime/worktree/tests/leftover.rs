//! Tests of what is already at the worktree's path before a run: a folder, someone's own
//! worktree, a worktree git lists whose folder is gone. The run stops before it makes anything,
//! leaves each alone, and its fix line's commands clear it without losing work.

use super::super::*;
use super::{Fake, git, left_behind, measure_with, repo};
use crate::process::{self, Cmd, Program, TEST_TIMEOUT};
use crate::scratch::Scratch;
use std::fs;

#[test]
fn a_folder_already_at_the_worktree_path_stops_the_run_and_is_left_alone() {
    let scratch = Scratch::new("worktree-there");
    let root = repo(&scratch);
    let path = place(&root).expect("a place");
    Scratch::create_dir_all(&path).expect("a folder in the way");
    Scratch::write(path.join("notes.txt"), "kept").expect("a file in it");
    let mut fake = Fake::default();
    let refused = measure_with(&root, false, 1, &mut fake).expect_err("refused");
    let shown = path.display();
    assert_eq!(
        refused.message,
        format!(
            "{shown} is already there; an interrupted `cargo xtask buildtime worktree` run may \
             have left it"
        )
    );
    assert_eq!(refused.fix, already_there_fix(&path));
    assert_eq!(fake.ran("git worktree"), 0);
    assert_eq!(
        fs::read_to_string(path.join("notes.txt")).expect("kept"),
        "kept"
    );
}

/// The fix of a folder already at the worktree's path.
fn already_there_fix(path: &Path) -> String {
    format!(
        "if `git worktree list` names it, remove it with `git worktree remove {0}`, which \
         refuses a worktree with modified or untracked files; otherwise move it away. xtask never \
         deletes what this run did not make",
        path.display()
    )
}

/// Someone's own worktree at the path: the run leaves it alone, and the fix line's command keeps
/// their changes. `git worktree remove` without `--force` refuses a worktree with an untracked
/// file and removes one that holds only ignored files (git 2.54).
#[test]
fn a_worktree_already_at_the_path_is_left_alone_and_the_fix_keeps_its_changes() {
    let scratch = Scratch::new("worktree-someones");
    let root = repo(&scratch);
    let path = place(&root).expect("a place");
    let shown = path.to_string_lossy().into_owned();
    git(&root, &["worktree", "add", "--detach", &shown, "HEAD"]);
    Scratch::write(root.join(".git/info/exclude"), "/ignored.txt\n").expect("an ignore rule");
    Scratch::write(path.join("notes.txt"), "work").expect("an untracked file");
    let mut fake = Fake::default();
    let refused = measure_with(&root, false, 1, &mut fake).expect_err("refused");
    assert_eq!(refused.fix, already_there_fix(&path));
    assert_eq!(fake.ran("git worktree"), 0, "xtask removed nothing");
    let remove = |root: &Path| {
        let args = ["worktree", "remove", shown.as_str()];
        process::run_quiet(&Cmd::new(Program::Git, args, TEST_TIMEOUT), root).expect("git")
    };
    let refused = remove(&root);
    assert!(
        !refused.success()
            && refused
                .stderr
                .contains("contains modified or untracked files"),
        "{}",
        refused.stderr
    );
    assert_eq!(
        fs::read_to_string(path.join("notes.txt")).expect("kept"),
        "work"
    );
    Scratch::remove_file(path.join("notes.txt")).expect("the work moved away");
    Scratch::write(path.join("ignored.txt"), "build output").expect("an ignored file");
    assert!(remove(&root).success(), "only ignored files: removed");
    assert!(!left_behind(&root, &path));
}

/// A worktree git lists whose folder is gone stops the run before anything is made: removing
/// that registration is not this run's to do, and `git worktree add` would fail on it. The fix
/// line's commands clear it, a locked one too.
#[test]
fn a_registered_worktree_whose_folder_is_gone_stops_the_run_and_the_fix_clears_it() {
    let scratch = Scratch::new("worktree-registered");
    let root = repo(&scratch);
    let path = place(&root).expect("a place");
    let shown = path.to_string_lossy().into_owned();
    git(&root, &["worktree", "add", "--detach", &shown, "HEAD"]);
    git(&root, &["worktree", "lock", &shown]);
    Scratch::remove_dir_all(&path).expect("the folder is gone");
    let mut fake = Fake::default();
    let refused = measure_with(&root, false, 1, &mut fake).expect_err("refused");
    assert_eq!(
        refused,
        Failure::new(
            format!(
                "`git worktree list` names {shown}, whose folder is gone; an interrupted \
                 `cargo xtask buildtime worktree` run may have left it"
            ),
            format!(
                "run `git worktree prune`, which forgets every worktree whose folder is gone \
                 unless it is locked: first `git worktree unlock {shown}` if \
                 `git worktree list` shows it locked; then run again"
            ),
        )
    );
    assert_eq!(fake.ran("cargo"), 0, "no warm-up");
    assert_eq!(fake.ran("git worktree add"), 0);
    assert_eq!(
        fake.ran("git worktree remove"),
        0,
        "the registration is kept"
    );
    git(&root, &["worktree", "prune"]);
    assert!(left_behind(&root, &path), "prune keeps a locked one");
    git(&root, &["worktree", "unlock", &shown]);
    git(&root, &["worktree", "prune"]);
    assert!(!left_behind(&root, &path), "unlocked, prune forgets it");
}
