//! Tests of what is already at the worktree's path before a run: a folder, someone's own
//! worktree, a worktree git lists whose folder is gone, what an interrupted run leaves. The run
//! stops before it makes anything, leaves each alone, and the one fix line's commands clear it
//! without losing work. Also a folder that comes during the warm-up, and a `git worktree list`
//! that fails after a run.

use super::super::*;
use super::{Fake, Trick, app_only, find, git, left_behind, measure_with, repo};
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
    assert_eq!(refused.fix, leftover_fix(&path));
    assert_eq!(fake.ran("git worktree"), 0);
    assert_eq!(
        fs::read_to_string(path.join("notes.txt")).expect("kept"),
        "kept"
    );
}

#[test]
fn the_leftover_fix_says_each_step_in_order() {
    let path = Path::new("/w/repo-buildtime-worktree");
    assert_eq!(
        leftover_fix(path),
        "run `git worktree list` from the repo root and clear /w/repo-buildtime-worktree in this \
         order: if it shows /w/repo-buildtime-worktree locked, `git worktree unlock \
         /w/repo-buildtime-worktree`; if it shows it prunable or its folder is gone, `git \
         worktree prune`, which forgets every worktree whose folder is gone unless it is locked; \
         if it still names /w/repo-buildtime-worktree, `git worktree remove \
         /w/repo-buildtime-worktree`, which refuses a worktree with modified or untracked files, \
         and when that refuses and the folder holds only a checkout of HEAD and target/ (what \
         this command makes), `git worktree remove --force /w/repo-buildtime-worktree`; move \
         away a folder left there that git does not name (delete it only if it holds nothing but \
         a checkout of HEAD and target/). xtask never deletes what this run did not make; then \
         run again"
    );
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
    assert_eq!(refused.fix, leftover_fix(&path));
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
            leftover_fix(&path),
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

/// The path is checked again right before each add: a folder someone made during the warm-up
/// fails `worktree_add` before git runs, is left alone, and no run follows.
#[test]
fn a_folder_made_during_the_warm_up_fails_the_add_before_git_runs_and_is_left_alone() {
    let scratch = Scratch::new("worktree-during-warm-up");
    let root = repo(&scratch);
    let mut fake = Fake {
        builds: vec![app_only()],
        tricks: vec![Trick::FolderDuringWarmUp],
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 2, &mut fake).expect("measured");
    let path = place(&root).expect("a place");
    let shown = path.display();
    assert_eq!(
        find(&measured, "worktree_add").outcome,
        Outcome::Failed(Failure::new(
            format!(
                "{shown} is already there; an interrupted `cargo xtask buildtime worktree` run \
                 may have left it"
            ),
            leftover_fix(&path)
        ))
    );
    assert_eq!(fake.ran("git worktree add"), 0, "git never ran the add");
    assert_eq!(fake.ran("git worktree remove"), 0);
    assert!(measured.errors.is_empty(), "nothing to remove");
    assert_eq!(
        find(&measured, "worktree_copy").outcome,
        Outcome::Skipped(NOT_REACHED.into())
    );
    assert!(!measured.passed(false));
    assert_eq!(
        fs::read_to_string(path.join("theirs.txt")).expect("kept"),
        "theirs"
    );
}

/// When `git worktree list` fails after a run and nothing is on disk, the failure says only
/// that: whether git still lists the worktree is unknown, so nothing is called left. With a
/// folder on disk, something is left, and the failed listing is one more reason.
#[test]
fn a_listing_that_fails_after_a_run_says_only_that_it_could_not_check() {
    let scratch = Scratch::new("worktree-list-fails");
    let root = repo(&scratch);
    let path = place(&root).expect("a place");
    let mut fake = Fake {
        builds: vec![app_only()],
        tricks: vec![Trick::ListFailsAfterAdd],
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 1, &mut fake).expect("measured");
    assert_eq!(
        measured.errors,
        [Failure::new(
            format!(
                "xtask could not check that `git worktree list` no longer names {}: `git \
                 worktree list --porcelain` failed",
                path.display()
            ),
            leftover_fix(&path)
        )]
    );
    assert!(!left_behind(&root, &path));
    let scratch = Scratch::new("worktree-list-fails-folder");
    let root = repo(&scratch);
    let mut fake = Fake {
        builds: vec![app_only()],
        tricks: vec![Trick::ListFailsAfterAdd, Trick::AddFailsAfterMakingIt],
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 1, &mut fake).expect("measured");
    let [failure] = measured.errors.as_slice() else {
        panic!("one failure: {:?}", measured.errors);
    };
    let shown = place(&root).expect("a place").display().to_string();
    assert!(
        failure.message.starts_with(&format!(
            "this run's `git worktree add` failed and something is left at {shown}: its folder \
             is on disk; `git worktree list --porcelain` failed. "
        )),
        "{}",
        failure.message
    );
}

/// What git 2.54 leaves when a run is killed (SIGKILL, so git cannot clean up), each tried by
/// hand on git 2.54: during `git worktree add`, a worktree locked as `initializing` with part of
/// the checkout; during `git worktree remove --force`, a worktree missing tracked files, or, once
/// git deleted its `.git` file, a folder git lists as prunable. Each stops the next run, and the
/// fix line's steps, in its order, clear each.
#[test]
fn the_leftover_fix_clears_what_an_interrupted_add_or_remove_leaves() {
    let attempt = |root: &Path, args: &[&str]| {
        let ran = process::run_quiet(&Cmd::new(Program::Git, args, TEST_TIMEOUT), root);
        let ran = ran.expect("git");
        (ran.success(), ran.stderr)
    };
    for case in ["locked", "missing files", "no .git"] {
        let scratch = Scratch::new("worktree-interrupted");
        let root = repo(&scratch);
        Scratch::write(root.join(".git/info/exclude"), "/target/\n").expect("target ignored");
        let path = place(&root).expect("a place");
        let shown = path.to_string_lossy().into_owned();
        git(&root, &["worktree", "add", "--detach", &shown, "HEAD"]);
        Scratch::create_dir_all(path.join("target/debug")).expect("copied output");
        Scratch::remove_file(path.join("src/lib.rs")).expect("a tracked file not there");
        let admin = root
            .join(".git/worktrees")
            .join(path.file_name().expect("a name"));
        match case {
            "locked" => Scratch::write(admin.join("locked"), "initializing").expect("locked"),
            "no .git" => Scratch::remove_file(path.join(".git")).expect("no .git"),
            _ => {}
        }
        let mut fake = Fake::default();
        let refused = measure_with(&root, false, 1, &mut fake).expect_err("refused");
        assert_eq!(refused.fix, leftover_fix(&path), "{case}");
        let listed = git(&root, &["worktree", "list"]).stdout;
        let force = ["worktree", "remove", "--force", shown.as_str()];
        if case == "locked" {
            assert!(listed.contains("locked"), "{listed}");
            let (removed, said) = attempt(&root, &force);
            assert!(!removed, "--force alone does not remove a locked one");
            assert!(
                said.contains("cannot remove a locked working tree"),
                "{said}"
            );
            git(&root, &["worktree", "unlock", &shown]);
        }
        if case == "no .git" {
            assert!(listed.contains("prunable"), "{listed}");
            let (removed, said) = attempt(&root, &force);
            assert!(!removed, "--force cannot remove one without its .git");
            assert!(said.contains("validation failed"), "{said}");
            git(&root, &["worktree", "prune"]);
            assert!(!git(&root, &["worktree", "list"]).stdout.contains(&shown));
            Scratch::remove_dir_all(&path).expect("the folder git no longer names, moved away");
        } else {
            let (removed, said) = attempt(&root, &["worktree", "remove", &shown]);
            assert!(!removed, "{case}: plain remove refuses");
            assert!(
                said.contains("contains modified or untracked files"),
                "{said}"
            );
            git(&root, &["worktree", "remove", "--force", &shown]);
        }
        assert!(!left_behind(&root, &path), "{case}: cleared");
    }
}
