//! The live watch's stage F rules (`live.rs`): kill first, a scan's budget, a path gone, a
//! folder that becomes a link, what a launch `commondir` names, the notes folder's watch, and
//! the restore that never puts a repository back.

use std::path::{Path, PathBuf};

use super::links::link;
use super::live::{calm, origins, reported, watched};
use super::{after, before, exit, launch, volume, world};
use crate::SetAsideWhat;
use crate::windows::set_aside::live::{Ending, Watched};
use crate::windows::set_aside::{Reason, SetAsideFailure, run};
use crate::windows::test_world::matter_one;
use crate::windows::write::rename_new;

/// Kill first: the session is ended before the first move, once for the check, and a check that
/// moves nothing ends nothing.
#[test]
fn the_session_ends_before_the_first_move_and_only_when_something_moves() {
    let (w, root, data) = world("live-kill-first");
    w.dir.file(
        "matter/Forms/hooks/pre-commit",
        "#!/bin/sh
",
    );
    w.dir.file("matter/notes.md", "");
    let watch = watched(&root, &data);

    let hooks = calm();
    watch.changed(&volume(), &reported(&["notes.md"]), exit(), &hooks);
    assert_eq!(*hooks.ends.borrow(), [], "nothing to move");

    rename_new(&root.join("Forms"), &root.join(".git")).unwrap();
    let live = watch.changed(&volume(), &reported(&[".git"]), exit(), &hooks);
    assert_eq!(origins(&live), [root.join(".git")]);
    assert_eq!(*hooks.ends.borrow(), [Ending::Found]);
}

/// A reported `.git` already gone is listed, so the watch can count a path renamed back and
/// forth.
#[test]
fn a_reported_git_already_gone_is_listed_gone() {
    let (_w, root, data) = world("live-gone");
    let watch = watched(&root, &data);
    let hooks = calm();
    let live = watch.changed(&volume(), &reported(&["sub/.git"]), exit(), &hooks);
    assert_eq!(live.gone, [PathBuf::from("sub/.git")]);
    assert!(live.moved.is_empty() && hooks.ends.borrow().is_empty());
}

/// H1: an empty folder the agent made, then turned into a junction (`mklink /J`'s two steps):
/// its attribute change is checked as a link made. One leading out goes; one within stays, and
/// a plain folder's change scans nothing.
#[test]
fn a_folder_turned_into_a_link_leading_out_is_set_aside() {
    let (w, root, data) = world("live-relinked");
    w.dir.file(
        "temp/repo/.git/HEAD",
        "ref: refs/heads/main
",
    );
    w.dir.file("matter/inside/keep.txt", "");
    w.dir.file(
        "matter/plain/.git/HEAD",
        "ref: refs/heads/main
",
    );
    let watch = watched(&root, &data);

    // The stand-in can't turn a folder into a link in place; a link made at the same name is
    // what the attribute change leaves.
    link(&w, "matter/out", &w.canonical("temp/repo"));
    link(&w, "matter/within", &root.join("inside"));
    let hooks = calm();
    let paths = reported(&["out", "within", "plain"]);
    let live = watch.relinked(&volume(), &paths, exit(), &hooks);

    assert_eq!(origins(&live), [root.join("out")]);
    assert_eq!(live.moved[0].what, SetAsideWhat::LinkOut);
    assert!(std::fs::symlink_metadata(root.join("within")).is_ok());
    assert!(
        root.join("plain/.git/HEAD").is_file(),
        "a launch .git, never scanned here"
    );
    assert_eq!(*hooks.ends.borrow(), [Ending::Found]);
}

/// `objects` and `refs` that are links git follows to folders make a git folder too.
#[test]
fn a_new_folder_with_linked_objects_and_refs_is_set_aside_whole() {
    let (w, root, data) = world("live-linked-layout");
    w.dir.file("matter/store/objects/info/packs", "");
    w.dir.file(
        "matter/store/refs/heads/main",
        "0123
",
    );
    let watch = watched(&root, &data);

    w.dir.file(
        "matter/work/repo/HEAD",
        "ref: refs/heads/main
",
    );
    link(&w, "matter/work/repo/objects", &root.join("store/objects"));
    link(&w, "matter/work/repo/refs", &root.join("store/refs"));
    let live = watch.changed(&volume(), &reported(&["work"]), exit(), &calm());

    assert_eq!(origins(&live), [root.join("work/repo")]);
}

/// 9a: a launch `commondir` naming a folder that is missing: the folder made there goes whole.
/// One naming the matter's own folder: git's names made at the root go, the rest stay.
#[test]
fn what_git_reads_where_a_launch_commondir_points_is_set_aside() {
    let (w, root, data) = world("live-common");
    w.dir.file(
        "matter/wt/HEAD",
        "ref: refs/heads/main
",
    );
    w.dir.file(
        "matter/wt/commondir",
        "../common
",
    );
    w.dir.file(
        "matter/top/HEAD",
        "ref: refs/heads/main
",
    );
    w.dir.file(
        "matter/top/commondir",
        "..
",
    );
    let watch = watched(&root, &data);

    w.dir.file(
        "matter/common/config",
        "[core]
	fsmonitor = evil
",
    );
    w.dir.file(
        "matter/hooks/post-checkout",
        "#!/bin/sh
",
    );
    w.dir.file("matter/brief.md", "");
    let paths = reported(&["common", "hooks", "brief.md"]);
    let live = watch.changed(&volume(), &paths, exit(), &calm());

    assert_eq!(origins(&live), [root.join("common"), root.join("hooks")]);
    assert!(root.join("brief.md").is_file());
}

/// 9a, the after-session pass: what is made where a launch `commondir` points is checked too.
#[test]
fn the_after_session_pass_checks_where_a_launch_commondir_points() {
    let (w, root, data) = world("exit-common");
    w.dir.file(
        "matter/wt/HEAD",
        "ref: refs/heads/main
",
    );
    w.dir.file(
        "matter/wt/commondir",
        "..
",
    );
    w.dir.file("matter/info/lawyers.txt", "");
    before(&root, &data, launch()).unwrap();

    w.dir.file(
        "matter/config",
        "[core]
	fsmonitor = evil
",
    );
    let moved = after(&root, &data, exit()).unwrap();
    let from: Vec<PathBuf> = moved.iter().map(|e| e.from.clone()).collect();
    assert_eq!(from, [root.join("config")]);
    assert!(
        root.join("info/lawyers.txt").is_file(),
        "there at launch: stays"
    );
}

/// The notes folder's watch: what was there when it started stays; a `.git` made goes into the
/// matter's set-aside.
#[test]
fn the_notes_folders_watch_moves_a_new_git_into_the_matters_set_aside() {
    let (w, root, data) = world("live-notes");
    w.dir.file(
        "notes/.git/HEAD",
        "ref: refs/heads/main
",
    );
    w.dir.file("notes/today.md", "");
    let notes = w.canonical("notes");
    before(&root, &data, launch()).unwrap();
    let watch = Watched::new_scanned(&volume(), &notes, &data, &matter_one());
    assert_eq!(watch.root(), notes.as_path());

    w.dir.file(
        "notes/sub/.git/HEAD",
        "ref: refs/heads/main
",
    );
    let live = watch.changed(&volume(), &reported(&[".git", "sub"]), exit(), &calm());
    assert_eq!(origins(&live), [notes.join("sub/.git")]);
    assert_eq!(live.moved[0].entry.matter, matter_one());
    assert!(notes.join(".git/HEAD").is_file());
}

/// M2: restore refuses a set-aside folder holding a `HEAD` (a file or a link) at any depth, or
/// a `commondir`, and a lone `HEAD`; a folder with none comes back.
#[test]
fn restore_refuses_anything_holding_a_head_or_a_commondir() {
    let (w, root, data) = world("live-restore-bare");
    w.dir.file(
        "matter/a/deep/er/HEAD",
        "ref: refs/heads/main
",
    );
    w.dir.file(
        "matter/b/x/commondir",
        "..
",
    );
    w.dir.file("matter/c/keep.txt", "");
    w.dir.file(
        "matter/e/HEAD",
        "ref: refs/heads/main
",
    );
    w.dir.file("matter/d/y/keep.txt", "");
    link(&w, "matter/d/y/HEAD", &root.join("c"));
    before(&root, &data, launch()).unwrap();
    let aside = |rel: &str| {
        run::set_aside_path(
            &volume(),
            &data,
            &matter_one(),
            &root.join(rel),
            Reason::New,
            exit(),
        )
        .unwrap()
    };
    for rel in ["a", "b", "d", "e/HEAD"] {
        let entry = aside(rel);
        let refused = run::restore(&volume(), &data, &entry).unwrap_err();
        assert_eq!(refused.failure, SetAsideFailure::GitFolder, "{rel}");
    }
    let plain = aside("c");
    run::restore(&volume(), &data, &plain).unwrap();
    assert!(Path::new(&root.join("c/keep.txt")).is_file());
}
