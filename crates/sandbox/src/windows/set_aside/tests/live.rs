//! The live watch's half (`live.rs`): what the outer runner's watch reports, checked and moved
//! through the after-session pass's own move, and the restore that never puts a `.git` back.

use std::path::{Path, PathBuf};

use super::links::link;
use super::{aside, exit, key, launch, launch_as, list, none, volume, world};
use crate::SetAsideWhat;
use crate::windows::set_aside::live::{Ending, Failed, Hooks, Live, Watched};
use crate::windows::set_aside::{Entry, Reason, SetAsideFailure, run};
use crate::windows::test_world::matter_one;
use crate::windows::write::rename_new;

/// The watch over the matter at `root`, after session `s1`'s launch pass recorded it.
pub(super) fn watched(root: &Path, data: &Path) -> Watched {
    assert_eq!(
        launch_as(&volume(), root, data, "s1", &[], launch()).unwrap(),
        none()
    );
    Watched::new(&volume(), root, data, &matter_one())
}

/// The watch's side of a check: each end it was asked for, and a clock that moves
/// `step` at each reading.
pub(super) struct Calm {
    pub(super) ends: std::cell::RefCell<Vec<Ending>>,
    pub(super) clock: std::cell::Cell<std::time::Duration>,
    step: std::time::Duration,
}

impl Hooks for Calm {
    fn end(&self, why: Ending) {
        self.ends.borrow_mut().push(why);
    }

    fn now(&self) -> std::time::Duration {
        let now = self.clock.get();
        self.clock.set(now + self.step);
        now
    }
}

/// Hooks whose clock stands still.
pub(super) fn calm() -> Calm {
    ticking(std::time::Duration::ZERO)
}

/// Hooks whose clock moves `step` at each reading.
pub(super) fn ticking(step: std::time::Duration) -> Calm {
    Calm {
        ends: std::cell::RefCell::new(Vec::new()),
        clock: std::cell::Cell::new(std::time::Duration::ZERO),
        step,
    }
}

/// The relative paths the watch reported, parts joined by `/`.
pub(super) fn reported(paths: &[&str]) -> Vec<PathBuf> {
    paths.iter().map(|rel| key(rel).iter().collect()).collect()
}

/// What `live` moved, as the manifest lists it.
fn entries(live: &Live) -> Vec<Entry> {
    live.moved.iter().map(|m| m.entry.clone()).collect()
}

/// Where each item `live` moved was, sorted, after checking nothing failed.
pub(super) fn origins(live: &Live) -> Vec<PathBuf> {
    assert_eq!(live.failed, Vec::<Failed>::new());
    let mut origins: Vec<PathBuf> = live.moved.iter().map(|m| m.entry.from.clone()).collect();
    origins.sort();
    origins
}

/// The hole: the agent renames a lawyer's folder to `.git`; it stays the lawyer's, so git would
/// trust it. The rename's report moves it at once, as the after-session pass would.
#[test]
fn a_rename_to_git_is_set_aside_at_once() {
    let (w, root, data) = world("live-rename");
    w.dir
        .file("matter/Precedents/hooks/post-checkout", "#!/bin/sh\n");
    let watch = watched(&root, &data);

    rename_new(&root.join("Precedents"), &root.join(".git")).unwrap();
    let live = watch.changed(&volume(), &reported(&[".git"]), exit(), &calm());

    assert_eq!(origins(&live), [root.join(".git")]);
    let moved = &live.moved[0];
    assert_eq!(moved.entry.to, aside(&data, exit(), ".git"));
    assert_eq!(moved.entry.reason, Reason::New);
    assert_eq!(
        (moved.path.as_path(), moved.what),
        (Path::new(".git"), SetAsideWhat::GitRepository)
    );
    assert!(!root.join(".git").exists());
    assert!(moved.entry.to.join("hooks/post-checkout").is_file());
    assert_eq!(list(&data), entries(&live), "the manifest lists it");
}

/// A `.git` file (a gitfile naming a folder elsewhere) added below the root goes too.
#[test]
fn an_added_git_file_is_set_aside() {
    let (w, root, data) = world("live-git-file");
    w.dir.file("matter/sub/brief.md", "# the lawyer's\n");
    let watch = watched(&root, &data);

    w.dir.file("matter/sub/.git", "gitdir: ../../elsewhere\n");
    let live = watch.changed(&volume(), &reported(&["sub/.git"]), exit(), &calm());

    assert_eq!(origins(&live), [root.join("sub/.git")]);
    assert_eq!(live.moved[0].entry.within, key("sub"));
    assert!(root.join("sub/brief.md").is_file());
}

/// NTFS opens `.GIT` as `.git`, and so does git; HFS+ opens `.g<U+200C>it` as `.git`, and a
/// matter may sync to a Mac, so the shared strip (`strip_hfs_ignorables`) goes first.
#[test]
fn a_git_in_another_case_or_with_an_hfs_ignorable_is_set_aside() {
    let (w, root, data) = world("live-case");
    w.dir.file("matter/Drafts/hooks/pre-commit", "#!/bin/sh\n");
    w.dir.file("matter/Forms/hooks/pre-commit", "#!/bin/sh\n");
    let watch = watched(&root, &data);

    rename_new(&root.join("Drafts"), &root.join(".GIT")).unwrap();
    rename_new(&root.join("Forms"), &root.join(".g\u{200C}it")).unwrap();
    let live = watch.changed(
        &volume(),
        &reported(&[".GIT", ".g\u{200C}it"]),
        exit(),
        &calm(),
    );

    assert_eq!(
        origins(&live),
        [root.join(".GIT"), root.join(".g\u{200C}it")]
    );
    assert!(live.moved[1].entry.to.join("hooks/pre-commit").is_file());
}

/// Names that only start like `.git`, and a folder named like `HEAD`, are the lawyer's own.
#[test]
fn gitignore_github_and_a_head_folder_never_trigger() {
    let (w, root, data) = world("live-lookalikes");
    let watch = watched(&root, &data);

    w.dir.file("matter/.gitignore", "target\n");
    w.dir.file("matter/.github/workflows/ci.yml", "on: push\n");
    w.dir.file("matter/Head/notes.md", "");
    let paths = reported(&[".gitignore", ".github", ".github/workflows", "Head"]);
    let live = watch.changed(&volume(), &paths, exit(), &calm());

    assert!(live.moved.is_empty() && live.failed.is_empty(), "{live:?}");
    assert!(root.join(".github/workflows/ci.yml").is_file());
    assert_eq!(list(&data), none());
}

/// A `.git` the launch pass recorded is write-denied: the watch leaves it, and what the
/// lawyer's own git writes inside it, `HEAD` included.
#[test]
fn a_git_in_the_launch_snapshot_is_left() {
    let (w, root, data) = world("live-known");
    w.dir.file("matter/.git/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/sub/.git/HEAD", "ref: refs/heads/main\n");
    let watch = watched(&root, &data);

    w.dir.file("matter/sub/.git/objects/ab/cdef", "blob");
    let paths = reported(&[".git", "sub/.git", "sub/.git/objects/ab", "sub/.git/HEAD"]);
    let live = watch.changed(&volume(), &paths, exit(), &calm());
    let rescanned = watch.rescan(&volume(), exit(), &calm());

    assert!(live.moved.is_empty() && live.failed.is_empty(), "{live:?}");
    assert!(rescanned.moved.is_empty(), "{rescanned:?}");
    assert!(root.join("sub/.git/objects/ab/cdef").is_file());
}

/// The launch's `.git` swapped for another folder at the same name is caught by its file ID.
#[test]
fn a_git_swapped_at_a_known_name_is_set_aside_as_changed() {
    let (w, root, data) = world("live-swapped");
    w.dir.file("matter/sub/.git/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/drafts/hooks/pre-commit", "#!/bin/sh\n");
    let watch = watched(&root, &data);

    rename_new(&root.join("sub/.git"), &root.join("sub/old")).unwrap();
    rename_new(&root.join("drafts"), &root.join("sub/.git")).unwrap();
    let live = watch.changed(
        &volume(),
        &reported(&["sub/old", "sub/.git"]),
        exit(),
        &calm(),
    );

    assert_eq!(origins(&live), [root.join("sub/.git")]);
    assert_eq!(live.moved[0].entry.reason, Reason::Changed);
    assert!(root.join("sub/old/HEAD").is_file());
}

/// The watch reports only the name of a folder moved in, not the `.git` inside it: the agent
/// moves a lawyer's folder out to its temp folder, renames it to `.git` inside `foo` there, and
/// moves `foo` back. The subtree below a reported folder is scanned, at any depth.
#[test]
fn a_folder_holding_a_git_moved_in_is_caught_at_any_depth() {
    let (w, root, data) = world("live-moved-in");
    w.dir
        .file("matter/Precedents/hooks/post-checkout", "#!/bin/sh\n");
    w.dir.file("matter/Forms/hooks/pre-commit", "#!/bin/sh\n");
    w.dir.file("temp/foo/keep.txt", "");
    w.dir.file("temp/bar/deep/keep.txt", "");
    let temp = w.canonical("temp");
    let watch = watched(&root, &data);

    rename_new(&root.join("Precedents"), &temp.join("foo/.git")).unwrap();
    rename_new(&root.join("Forms"), &temp.join("bar/deep/.git")).unwrap();
    rename_new(&temp.join("foo"), &root.join("foo")).unwrap();
    rename_new(&temp.join("bar"), &root.join("bar")).unwrap();
    let live = watch.changed(&volume(), &reported(&["foo", "bar"]), exit(), &calm());

    assert_eq!(
        origins(&live),
        [root.join("bar/deep/.git"), root.join("foo/.git")]
    );
    assert!(root.join("foo/keep.txt").is_file() && root.join("bar/deep/keep.txt").is_file());
}

/// A clone into the matter: its folder's report finds the `.git` inside, made after the folder.
#[test]
fn a_cloned_tree_is_caught_by_its_folder() {
    let (w, root, data) = world("live-clone");
    let watch = watched(&root, &data);

    w.dir
        .file("matter/clone/.git/HEAD", "ref: refs/heads/main\n");
    w.dir
        .file("matter/clone/.git/hooks/post-checkout", "#!/bin/sh\n");
    w.dir.file("matter/clone/README.md", "# cloned\n");
    let live = watch.changed(
        &volume(),
        &reported(&["clone", "clone/.git/HEAD"]),
        exit(),
        &calm(),
    );

    assert_eq!(origins(&live), [root.join("clone/.git")]);
    assert!(root.join("clone/README.md").is_file());
}

/// Rule (a): a folder made or moved in whose subtree holds a bare repository (`HEAD`, `objects`,
/// `refs`, no `.git` name) goes whole.
#[test]
fn a_new_folder_holding_a_bare_repository_is_set_aside_whole() {
    let (w, root, data) = world("live-bare-new");
    let watch = watched(&root, &data);

    w.dir
        .file("matter/work/repo/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/work/repo/objects/info/packs", "");
    w.dir.file("matter/work/repo/refs/heads/main", "0123\n");
    w.dir
        .file("matter/work/repo/hooks/post-update", "#!/bin/sh\n");
    let live = watch.changed(&volume(), &reported(&["work"]), exit(), &calm());

    assert_eq!(origins(&live), [root.join("work/repo")]);
    assert!(live.moved[0].entry.to.join("hooks/post-update").is_file());
    assert!(root.join("work").is_dir() && !root.join("work/repo").exists());
}

/// Rule (b): the agent fills a lawyer's folder with a bare repository's parts. Its `HEAD` goes,
/// which ends git's discovery, and the lawyer's folder and files stay where they are; a rescan
/// does the same with no report.
#[test]
fn a_head_made_in_a_lawyers_folder_is_set_aside_alone() {
    let (w, root, data) = world("live-bare-old");
    w.dir.file("matter/Precedents/brief.md", "# the lawyer's\n");
    w.dir.file("matter/Forms/brief.md", "# the lawyer's\n");
    let watch = watched(&root, &data);

    for folder in ["Precedents", "Forms"] {
        w.dir
            .file(&format!("matter/{folder}/objects/info/packs"), "");
        w.dir
            .file(&format!("matter/{folder}/refs/heads/main"), "0123\n");
        w.dir
            .file(&format!("matter/{folder}/HEAD"), "ref: refs/heads/main\n");
    }
    let paths = reported(&["Precedents/objects", "Precedents/refs", "Precedents/HEAD"]);
    let live = watch.changed(&volume(), &paths, exit(), &calm());
    let rescanned = watch.rescan(&volume(), exit(), &calm());

    assert_eq!(origins(&live), [root.join("Precedents/HEAD")]);
    assert_eq!(origins(&rescanned), [root.join("Forms/HEAD")]);
    for folder in ["Precedents", "Forms"] {
        assert!(root.join(folder).join("brief.md").is_file());
        assert!(root.join(folder).join("objects").is_dir());
    }
}

/// A `HEAD` that is a link counts as one (git reads a symbolic `HEAD`), and a folder made with
/// `HEAD` and `commondir` is a worktree's git folder, which goes whole.
#[test]
fn a_linked_head_and_a_commondir_folder_are_git_parts() {
    let (w, root, data) = world("live-head-shapes");
    w.dir.file("matter/Precedents/brief.md", "# the lawyer's\n");
    w.dir.file("matter/target/keep.txt", "");
    let watch = watched(&root, &data);

    w.dir.file("matter/Precedents/objects/info/packs", "");
    w.dir.file("matter/Precedents/refs/heads/main", "0123\n");
    link(&w, "matter/Precedents/HEAD", &root.join("target"));
    w.dir.file("matter/wt/gitdir/HEAD", "0123\n");
    w.dir
        .file("matter/wt/gitdir/commondir", "../../elsewhere/.git\n");
    let live = watch.changed(
        &volume(),
        &reported(&["Precedents/HEAD", "wt"]),
        exit(),
        &calm(),
    );

    assert_eq!(
        origins(&live),
        [root.join("Precedents/HEAD"), root.join("wt/gitdir")]
    );
    assert!(root.join("Precedents/brief.md").is_file());
    let exit_scan = watch.rescan(&volume(), exit(), &calm());
    assert!(exit_scan.moved.is_empty(), "{exit_scan:?}");
}

/// A link made in the matter that leads out of it goes at once, as itself; one that leads
/// within the matter is left to the after-session pass.
#[test]
fn a_link_out_of_the_matter_is_set_aside_and_one_within_is_left() {
    let (w, root, data) = world("live-link");
    w.dir.file("matter/sub/notes.md", "");
    w.dir.file("temp/foo/.git/HEAD", "ref: refs/heads/main\n");
    let watch = watched(&root, &data);

    link(&w, "matter/out", &w.canonical("temp/foo"));
    link(&w, "matter/within", &root.join("sub"));
    let live = watch.changed(&volume(), &reported(&["out", "within"]), exit(), &calm());

    assert_eq!(origins(&live), [root.join("out")]);
    assert_eq!(live.moved[0].what, SetAsideWhat::LinkOut);
    assert!(std::fs::symlink_metadata(root.join("within")).is_ok());
    assert!(
        w.at("temp/foo/.git/HEAD").is_file(),
        "what it named is left alone"
    );
}

/// After an overflow the watch rescans the whole matter: a `.git` nobody reported is found.
#[test]
fn a_rescan_finds_a_git_nobody_reported() {
    let (w, root, data) = world("live-rescan");
    w.dir.file("matter/.git/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/a/b/notes.md", "");
    let watch = watched(&root, &data);

    w.dir
        .file("matter/a/b/.git/hooks/post-checkout", "#!/bin/sh\n");
    let live = watch.rescan(&volume(), exit(), &calm());

    assert_eq!(origins(&live), [root.join("a/b/.git")]);
    assert!(root.join(".git/HEAD").is_file(), "the launch's .git stays");
}

/// After the session ended, an item the watch couldn't move is tried again by its full path.
#[test]
fn a_retry_moves_what_failed_by_its_full_path() {
    let (w, root, data) = world("live-retry");
    w.dir.file("matter/Forms/hooks/pre-commit", "#!/bin/sh\n");
    w.dir.file("matter/a/keep.txt", "");
    let watch = watched(&root, &data);

    rename_new(&root.join("Forms"), &root.join("a/.git")).unwrap();
    let failed = |path: PathBuf| Failed {
        path,
        failure: SetAsideFailure::Io(std::io::ErrorKind::ResourceBusy),
        what: SetAsideWhat::GitRepository,
    };
    let elsewhere = w.at("elsewhere/.git");
    let tried = [failed(root.join("a/.git")), failed(elsewhere.clone())];
    let live = watch.retry(&volume(), &tried, exit(), &calm());

    let moved: Vec<_> = live.moved.iter().map(|m| m.entry.from.clone()).collect();
    assert_eq!(moved, [root.join("a/.git")]);
    let left: Vec<_> = live.failed.iter().map(|f| f.path.clone()).collect();
    assert_eq!(left, [elsewhere], "outside the matter");
}

/// Restore refuses a set-aside `.git` and anything inside one, and the item stays listed.
#[test]
fn restore_refuses_a_git_and_anything_inside_one() {
    let (w, root, data) = world("live-restore");
    w.dir
        .file("matter/Precedents/hooks/post-checkout", "#!/bin/sh\n");
    w.dir.file("matter/sub/.git/hooks/x", "#!/bin/sh\n");
    let watch = watched(&root, &data);
    rename_new(&root.join("Precedents"), &root.join(".git")).unwrap();
    let live = watch.changed(&volume(), &reported(&[".git"]), exit(), &calm());
    let inside = run::set_aside_path(
        &volume(),
        &data,
        &matter_one(),
        &root.join("sub/.git/hooks"),
        Reason::LawyerCantOpen,
        exit(),
    )
    .unwrap();

    for entry in [&live.moved[0].entry, &inside] {
        let refused = run::restore(&volume(), &data, entry).unwrap_err();
        assert_eq!(refused.failure, SetAsideFailure::GitFolder, "{entry:?}");
    }
    assert!(!root.join(".git").exists() && !root.join("sub/.git/hooks").exists());
    assert_eq!(list(&data).len(), 2, "both stay set aside");
    assert!(
        SetAsideFailure::GitFolder
            .to_string()
            .starts_with("Clauseo won't put back a git folder or repository an agent made")
    );
}
