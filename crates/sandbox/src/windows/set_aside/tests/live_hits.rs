//! Review items 1, 2, 5 and 7 over the stand-in volume: a reported `.git` or `HEAD` ends the
//! session gone or not, and is moved before any folder reported is scanned; an end that can't
//! confirm the job is empty moves nothing; an unknown git folder ends the session before its
//! `commondir` is read, and one an oplock holds counts as unreadable; the owner of a folder set
//! aside whole is read from the handle the move renames; and each repository moved says
//! whether the agent made it.

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

use super::live::{calm, origins, reported, ticking, watched};
use super::posix::{POOL_USER, Posix};
use super::{exit, world};
use crate::SetAsideWhat;
use crate::windows::set_aside::live::{Ending, Hooks};
use crate::windows::write::rename_new;

const HEAD: &str = "ref: refs/heads/main\n";

/// A `HEAD` reported gone ends the session; one in a launch `.git` the lawyer renamed (the
/// lawyer's git renaming `HEAD.lock`) doesn't, nor does a folder named `HEAD`.
#[test]
fn a_reported_head_ends_the_session_unless_a_launch_git_holds_it_or_it_is_a_folder() {
    let (w, root, data) = world("live-hit-head");
    w.dir.file("matter/Forms/.git/HEAD", HEAD);
    let watch = watched(&root, &data);
    let volume = Posix::new(true);

    let hooks = calm();
    let live = watch.changed(&volume, &reported(&["x/HEAD"]), exit(), &hooks);
    assert_eq!(live.gone, [PathBuf::from("x/HEAD")]);
    assert_eq!(*hooks.ends.borrow(), [Ending::Found]);

    rename_new(&root.join("Forms/.git"), &root.join("Forms/old")).unwrap();
    let hooks = calm();
    let live = watch.changed(&volume, &reported(&["Forms/old/HEAD"]), exit(), &hooks);
    assert!(live.moved.is_empty() && hooks.ends.borrow().is_empty());

    w.dir.dir("matter/notes/HEAD");
    let hooks = calm();
    let live = watch.changed(&volume, &reported(&["notes/HEAD"]), exit(), &hooks);
    assert!(live.moved.is_empty() && hooks.ends.borrow().is_empty());
}

/// A reported `.git` is moved before any reported folder is scanned, whatever the order of the
/// reports.
#[test]
fn name_hits_are_moved_before_any_folder_reported_is_scanned() {
    let (w, root, data) = world("live-hit-order");
    let watch = watched(&root, &data);
    w.dir.file("matter/big/deep/.git/HEAD", HEAD);
    w.dir.file("matter/a/.git/HEAD", HEAD);

    let live = watch.changed(
        &Posix::new(true),
        &reported(&["big", "a/.git"]),
        exit(),
        &calm(),
    );
    let order: Vec<PathBuf> = live.moved.iter().map(|m| m.path.clone()).collect();
    assert_eq!(order, ["a/.git", "big/deep/.git"].map(PathBuf::from));
}

/// The end can't confirm the agent's job is empty: nothing is moved, the item is left for the
/// check after the session, and the scan of the folder reported stops.
#[test]
fn with_the_end_unconfirmed_nothing_is_moved_and_the_item_is_left() {
    let (w, root, data) = world("live-hit-unsure");
    let watch = watched(&root, &data);
    w.dir.file("matter/a/.git/HEAD", HEAD);
    w.dir.file("matter/big/b/.git/HEAD", HEAD);

    let hooks = calm();
    hooks.empty.set(false);
    let paths = reported(&["a/.git", "big"]);
    let live = watch.changed(&Posix::new(true), &paths, exit(), &hooks);
    assert!(live.moved.is_empty() && live.failed.is_empty());
    assert_eq!(live.left, [root.join("a/.git")]);
    assert_eq!(*hooks.ends.borrow(), [Ending::Found]);
    assert!(root.join("a/.git").is_dir() && root.join("big/b/.git").is_dir());
}

/// Hooks that note how many files the volume had read at each end.
struct Reading<'a> {
    volume: &'a Posix,
    reads_at_end: RefCell<Vec<usize>>,
}

impl Hooks for Reading<'_> {
    fn end(&self, _why: Ending) -> bool {
        let reads = self.volume.reads.borrow().len();
        self.reads_at_end.borrow_mut().push(reads);
        true
    }

    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

/// Review item 2: an unknown git folder ends the session before anything in it is read, its
/// `commondir` (which an agent can hold an oplock on) included. In a rescan the `commondir` is
/// read after the end and only the `HEAD` moves; in a folder made, the whole folder moves unread.
#[test]
fn an_unknown_git_folder_ends_the_session_before_its_commondir_is_read() {
    let (w, root, data) = world("live-bare-first");
    let watch = watched(&root, &data);
    w.dir.file("matter/wt/HEAD", HEAD);
    w.dir.file("matter/wt/commondir", "../shared\n");

    let volume = Posix::new(true);
    let hooks = Reading {
        volume: &volume,
        reads_at_end: RefCell::new(Vec::new()),
    };
    let live = watch.rescan(&volume, exit(), &hooks);
    assert_eq!(*hooks.reads_at_end.borrow(), [0], "ended before any read");
    assert_eq!(*volume.reads.borrow(), [root.join("wt/commondir")]);
    assert_eq!(origins(&live), [root.join("wt/HEAD")]);

    w.dir.file("matter/new/HEAD", HEAD);
    w.dir.file("matter/new/commondir", "../shared\n");
    let volume = Posix::new(true);
    let hooks = Reading {
        volume: &volume,
        reads_at_end: RefCell::new(Vec::new()),
    };
    let live = watch.changed(&volume, &reported(&["new"]), exit(), &hooks);
    assert_eq!(origins(&live), [root.join("new")]);
    assert!(volume.reads.borrow().is_empty(), "moved whole, unread");
}

/// A `commondir` whose open finds an oplock's break under way (NTFS's
/// `FILE_COMPLETE_IF_OPLOCKED`) is unreadable: the scan doesn't wait, and the git folder it is
/// in is still set aside.
#[test]
fn a_commondir_an_oplock_holds_is_unreadable_and_its_head_still_goes() {
    let (w, root, data) = world("live-bare-oplock");
    let watch = watched(&root, &data);
    w.dir.file("matter/wt/HEAD", HEAD);
    w.dir.file("matter/wt/commondir", "..\n");
    let volume = Posix::new(true);
    volume.oplocked.borrow_mut().push(root.join("wt/commondir"));

    let live = watch.rescan(&volume, exit(), &calm());
    assert_eq!(*volume.reads.borrow(), [root.join("wt/commondir")]);
    assert_eq!(origins(&live), [root.join("wt/HEAD")]);
}

/// Review item 5: past the budget, the owner that decides whether a folder goes whole is the
/// one read from the handle the move renames, not one read by path before: a folder swapped
/// for the lawyer's between the two stays, and the reverse goes.
#[test]
fn past_its_budget_the_owner_read_on_the_moved_handle_decides() {
    let (w, root, data) = world("live-owner-handle");
    let watch = watched(&root, &data);
    for at in 0..40 {
        w.dir.file(&format!("matter/big/f{at}/keep.txt"), "");
    }
    let swapped = Posix::new(true);
    swapped.foreign(&root.join("big"));
    swapped
        .swapped
        .borrow_mut()
        .insert(root.join("big"), "uid lawyer".to_owned());
    let hooks = ticking(Duration::from_millis(500));
    let live = watch.changed(&swapped, &reported(&["big"]), exit(), &hooks);
    assert_eq!(*hooks.ends.borrow(), [Ending::Slow]);
    assert!(live.moved.is_empty() && live.failed.is_empty());
    assert!(root.join("big/f0/keep.txt").exists());

    let agents = Posix::new(true);
    agents
        .swapped
        .borrow_mut()
        .insert(root.join("big"), POOL_USER.to_owned());
    let hooks = ticking(Duration::from_millis(500));
    let live = watch.changed(&agents, &reported(&["big"]), exit(), &hooks);
    let last = live.moved.last().expect("the folder moved");
    assert_eq!(
        (last.entry.from.clone(), last.what, last.by_agent),
        (root.join("big"), SetAsideWhat::UncheckedFolder, true)
    );
    assert_eq!(last.entry.owner.as_deref(), Some(POOL_USER));
}

/// Review item 7: a `.git` a pool user owns is the agent's; a lawyer's folder the agent renamed
/// to `.git` isn't, so its notice gives the step to put it back.
#[test]
fn each_repository_moved_says_whether_the_agent_made_it() {
    let (w, root, data) = world("live-by-agent");
    w.dir.file("matter/Precedents/memo.md", "");
    let watch = watched(&root, &data);
    let volume = Posix::new(true);
    w.dir.file("matter/made/.git/HEAD", HEAD);
    volume.foreign(&root.join("made/.git"));
    rename_new(&root.join("Precedents"), &root.join("sub")).unwrap();
    w.dir.dir("matter/sub2");
    rename_new(&root.join("sub"), &root.join("sub2/.git")).unwrap();

    let paths = reported(&["made/.git", "sub2/.git"]);
    let live = watch.changed(&volume, &paths, exit(), &calm());
    let whose: Vec<(PathBuf, bool)> = live
        .moved
        .iter()
        .map(|m| (m.path.clone(), m.by_agent))
        .collect();
    assert_eq!(
        whose,
        [
            (PathBuf::from("made/.git"), true),
            (PathBuf::from("sub2/.git"), false)
        ]
    );
}
