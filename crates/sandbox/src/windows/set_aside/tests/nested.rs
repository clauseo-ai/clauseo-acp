//! N1: the protected names below the matter's root that the scan finds, recorded at launch and
//! set aside when new or changed, each at its own path in the pass's folder.

use super::{aside, exit, exit_as, key, launch, launch_as, none, record, volume, world};
use crate::windows::set_aside::Reason;
use crate::windows::write::rename_new;

/// N1: the agent fills a lawyer folder with hooks and renames it to `sub\.git`; the lawyer
/// still owns it, so git's ownership check would pass and its hooks run in the lawyer's git.
#[test]
fn a_lawyer_folder_renamed_to_a_nested_git_after_launch_is_set_aside_with_its_hooks() {
    let (w, root, data) = world("aside-nested-git");
    w.dir
        .file("matter/Precedents/hooks/post-checkout", "#!/bin/sh\n");
    w.dir.file("matter/sub/brief.md", "# the lawyer's\n");
    assert_eq!(
        launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap(),
        none()
    );

    rename_new(&root.join("Precedents"), &root.join("sub/.git")).unwrap();
    let moved = exit_as(&volume(), &root, &data, "s1", exit()).unwrap();

    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0].from, root.join("sub/.git"));
    assert_eq!(moved[0].to, aside(&data, exit(), "sub/.git"));
    assert_eq!(moved[0].within, ["sub"]);
    assert_eq!(moved[0].name, ".git");
    assert_eq!(moved[0].reason, Reason::New);
    assert!(!root.join("sub/.git").exists());
    assert!(root.join("sub/brief.md").is_file());
    assert!(moved[0].to.join("hooks/post-checkout").is_file());
}

/// N1: a nested `.git` there at launch and swapped for another folder is set aside as changed.
#[test]
fn a_nested_git_replaced_since_launch_is_set_aside_as_changed() {
    let (w, root, data) = world("aside-nested-changed");
    w.dir.file("matter/sub/.git/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/drafts/hooks/pre-commit", "#!/bin/sh\n");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();

    rename_new(&root.join("sub/.git"), &root.join("sub/old-git")).unwrap();
    rename_new(&root.join("drafts"), &root.join("sub/.git")).unwrap();
    let moved = exit_as(&volume(), &root, &data, "s1", exit()).unwrap();

    let seen: Vec<_> = moved.iter().map(|e| (e.from.clone(), e.reason)).collect();
    assert_eq!(seen, [(root.join("sub/.git"), Reason::Changed)]);
    assert!(moved[0].to.join("hooks/pre-commit").is_file());
    assert!(root.join("sub/old-git/HEAD").is_file());
}

/// N1: a nested name the record has and the scan no longer finds is dropped from the record,
/// and nothing is moved for it.
#[test]
fn a_nested_git_gone_since_launch_is_dropped_from_the_record() {
    let (w, root, data) = world("aside-nested-gone");
    w.dir.file("matter/sub/.git/HEAD", "ref: refs/heads/main\n");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    assert!(record(&data).names.contains_key(&key("sub/.git")));

    rename_new(&root.join("sub/.git"), &root.join("sub/git-old")).unwrap();
    assert_eq!(
        exit_as(&volume(), &root, &data, "s1", exit()).unwrap(),
        none()
    );

    assert!(!record(&data).names.contains_key(&key("sub/.git")));
    assert!(root.join("sub/git-old/HEAD").is_file());
}

/// N1: many nested `.git` folders set aside in one pass each keep their own path below the
/// pass's folder, so none clashes and none needs a `~2` name.
#[test]
fn nested_names_are_set_aside_at_their_own_paths_and_never_clash() {
    let (w, root, data) = world("aside-nested-many");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    w.dir.file("matter/a/.git/hooks/post-merge", "#!/bin/sh\n");
    w.dir
        .file("matter/b/c/.git/hooks/post-merge", "#!/bin/sh\n");
    w.dir.file("matter/b/.mcp.json", "{}");
    w.dir.file("matter/b/c/.claude/settings.json", "{}");

    let moved = exit_as(&volume(), &root, &data, "s1", exit()).unwrap();

    let mut names: Vec<_> = moved.iter().map(|e| e.to.clone()).collect();
    names.sort();
    let expect: Vec<_> = ["a/.git", "b/.mcp.json", "b/c/.claude", "b/c/.git"]
        .iter()
        .map(|rel| aside(&data, exit(), rel))
        .collect();
    assert_eq!(names, expect);
    assert!(
        moved
            .iter()
            .all(|e| !e.name.to_str().unwrap().contains('~')),
        "{moved:?}"
    );
    assert!(moved.iter().all(|e| e.reason == Reason::New));
    assert!(expect[3].join("hooks/post-merge").is_file());
}

/// N1: a `.git` outside the matter is never scanned or moved, and a root `.git` is one entry,
/// moved once.
#[test]
fn a_git_outside_the_matter_is_left_and_one_at_the_root_is_one_entry() {
    let (w, root, data) = world("aside-nested-root");
    let outside = w.dir.file("elsewhere/.git/HEAD", "ref: refs/heads/main\n");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    w.dir.file("matter/.git/hooks/post-checkout", "#!/bin/sh\n");

    let moved = exit_as(&volume(), &root, &data, "s1", exit()).unwrap();

    let seen: Vec<_> = moved.iter().map(|e| (e.from.clone(), e.reason)).collect();
    assert_eq!(seen, [(root.join(".git"), Reason::New)]);
    assert!(outside.is_file());
}

/// N1: a nested `.git` inside a protected folder set aside in the same pass goes with it, and
/// isn't tried again on its own.
#[test]
fn a_nested_name_inside_a_folder_set_aside_goes_with_it() {
    let (w, root, data) = world("aside-nested-inside");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    w.dir
        .file("matter/.claude/x/.git/HEAD", "ref: refs/heads/main\n");

    let moved = exit_as(&volume(), &root, &data, "s1", exit()).unwrap();

    let moved_from: Vec<_> = moved.iter().map(|e| e.from.clone()).collect();
    assert_eq!(moved_from, [root.join(".claude")]);
    assert!(moved[0].to.join("x/.git/HEAD").is_file());
    assert!(!record(&data).names.contains_key(&key(".claude/x/.git")));
}
