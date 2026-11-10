//! The `commondir` files the walk reads: one beside a `HEAD` in any git dir, inside a `.git` or
//! not, never through a link and never on a network path.

use super::*;

const HEAD: &str = "ref: refs/heads/main\n";

/// A `commondir` beside a `HEAD` anywhere in a `.git` names the folder git reads the
/// repository's `config` from: in `.git` itself, a worktree's folder, a submodule's git dir and
/// its worktrees. `../..` from a worktree is the `.git`, listed once. One with no `HEAD` beside
/// it isn't read, and one naming the matter itself is left out.
#[test]
fn every_git_dirs_commondir_names_its_folder_inside_the_matter() {
    let dir = TempDir::new("links-commondir-git");
    for (folder, line) in [
        (".git", "../shared1"),
        (".git/worktrees/wt1", "../.."),
        (".git/worktrees/wt2", "../../../shared2"),
        (".git/modules/m", "../../../shared3"),
        (".git/modules/m/worktrees/wt", "../../../../../shared4"),
        (".git/worktrees/wt4", "../../../"),
    ] {
        dir.file(&format!("matter/{folder}/HEAD"), HEAD);
        dir.file(&format!("matter/{folder}/commondir"), &format!("{line}\n"));
    }
    dir.file("matter/.git/worktrees/wt3/commondir", "../../../shared5\n");
    for shared in ["shared1", "shared2", "shared3", "shared4", "shared5"] {
        dir.dir(&format!("matter/{shared}"));
    }
    let scan = scan_matter(&dir.path().join("matter")).unwrap();
    let matter = canon(&dir.path().join("matter"));
    let want = [".git", "shared1", "shared2", "shared3", "shared4"].map(|f| matter.join(f));
    assert_eq!(scan.git_dirs(), want);
    assert_eq!(scan.git(), [matter.join(".git")]);
    assert!(
        scan.git_dirs_outside.is_empty(),
        "{:?}",
        scan.git_dirs_outside
    );
}

/// A folder a `commondir` or `gitdir:` names outside the matter isn't a git dir in it; it is
/// kept apart for the policy, which refuses one the agent can write.
#[test]
fn a_folder_named_outside_the_matter_is_kept_for_the_policy() {
    let dir = TempDir::new("links-commondir-out");
    dir.file("matter/.git/worktrees/wt/HEAD", HEAD);
    dir.file(
        "matter/.git/worktrees/wt/commondir",
        "../../../../outside/common\n",
    );
    dir.dir("outside/common");
    dir.file("matter/b/.git", "gitdir: ../../outside/repo\n");
    dir.dir("outside/repo");
    dir.file("matter/c/HEAD", HEAD);
    dir.file("matter/c/commondir", "../../outside/bare\n");
    dir.dir("outside/bare");
    let scan = scan_matter(&dir.path().join("matter")).unwrap();
    assert!(scan.git_dirs().is_empty(), "{:?}", scan.git_dirs());
    let outside = ["bare", "common", "repo"].map(|f| canon(&dir.path().join("outside").join(f)));
    assert_eq!(scan.git_dirs_outside, outside);
}

/// A `commondir` beside a `HEAD` that is a link isn't followed: it is kept for the policy to
/// refuse, in a `.git` or outside one. One with no `HEAD` beside it isn't.
#[cfg(unix)]
#[test]
fn a_commondir_link_is_kept_unread() {
    let dir = TempDir::new("links-commondir-link");
    dir.dir("matter/shared");
    dir.file("matter/real", "../shared\n");
    dir.file("matter/.git/HEAD", HEAD);
    dir.symlink("matter/.git/commondir", Path::new("../real"));
    dir.file("matter/r/HEAD", HEAD);
    dir.symlink("matter/r/commondir", Path::new("../real"));
    dir.dir("matter/n");
    dir.symlink("matter/n/commondir", Path::new("../real"));
    let scan = scan_matter(&dir.path().join("matter")).unwrap();
    let matter = canon(&dir.path().join("matter"));
    assert!(scan.git_dirs().is_empty(), "{:?}", scan.git_dirs());
    assert_eq!(
        scan.commondir_links,
        [matter.join(".git/commondir"), matter.join("r/commondir")]
    );
}

/// The world for a `worktrees` folder that is a link out of the matter: `outside/real/wt`
/// holds a `HEAD` and a `commondir` naming `matter/other`.
fn linked_worktrees(test: &str) -> TempDir {
    let dir = TempDir::new(test);
    dir.file("outside/real/wt/HEAD", HEAD);
    dir.file("outside/real/wt/commondir", "../../../matter/other\n");
    dir.dir("matter/other");
    dir.dir("matter/.git");
    dir
}

/// Nothing a linked `worktrees` folder holds is read.
#[track_caller]
fn nothing_read_through(dir: &TempDir) {
    let scan = scan_matter(&dir.path().join("matter")).unwrap();
    let matter = canon(&dir.path().join("matter"));
    assert!(scan.git_dirs().is_empty(), "{:?}", scan.git_dirs());
    assert!(
        scan.git_dirs_outside.is_empty(),
        "{:?}",
        scan.git_dirs_outside
    );
    assert!(
        scan.commondir_links.is_empty(),
        "{:?}",
        scan.commondir_links
    );
    assert_eq!(scan.git(), [matter.join(".git")]);
}

/// A `worktrees` folder that is a symbolic link is not followed inside a `.git`.
#[cfg(unix)]
#[test]
fn a_symlinked_worktrees_folder_in_a_git_dir_is_not_read() {
    let dir = linked_worktrees("links-wt-link");
    dir.symlink("matter/.git/worktrees", &dir.path().join("outside/real"));
    nothing_read_through(&dir);
}

/// A `worktrees` folder that is a junction is not followed inside a `.git`.
#[cfg(windows)]
#[test]
fn a_junctioned_worktrees_folder_in_a_git_dir_is_not_read() {
    let dir = linked_worktrees("links-wt-junction");
    crate::windows::test_junction::junction(
        &dir.path().join("matter/.git/worktrees"),
        &dir.path().join("outside/real"),
    )
    .unwrap();
    nothing_read_through(&dir);
}

/// The walk skips a network `gitdir:` and network `commondir` files unread, in a `.git` or
/// beside a `HEAD` outside one.
#[test]
fn the_walk_lists_nothing_a_network_path_names() {
    let dir = TempDir::new("links-commondir-unc");
    dir.file("matter/a/.git", "gitdir: //host/share/repo\n");
    dir.file("matter/.git/HEAD", HEAD);
    dir.file("matter/.git/commondir", "\\\\host\\share\n");
    dir.file("matter/b/HEAD", HEAD);
    dir.file("matter/b/commondir", "\\\\?\\UNC\\host\\share\n");
    let scan = scan_matter(&dir.path().join("matter")).unwrap();
    assert!(scan.git_dirs().is_empty(), "{:?}", scan.git_dirs());
    assert!(
        scan.git_dirs_outside.is_empty(),
        "{:?}",
        scan.git_dirs_outside
    );
}
