//! N1: the protected names below the matter's root that the scan finds, recorded at launch and
//! set aside when new or changed, each at its own path in the pass's folder. S15 `PR4a` adds the
//! instruction files Claude and Codex load from the folder they work in.

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

/// `PR4a`: a nested `CLAUDE.md` the agent makes during a session is set aside, at its own path.
#[test]
fn a_nested_claude_md_made_during_a_session_is_set_aside() {
    let (w, root, data) = world("aside-nested-claude-md");
    w.dir.file("matter/a/b/brief.md", "# the lawyer's\n");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    w.dir.file("matter/a/b/CLAUDE.md", "# the agent's\n");

    let moved = exit_as(&volume(), &root, &data, "s1", exit()).unwrap();

    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0].from, root.join("a/b/CLAUDE.md"));
    assert_eq!(moved[0].to, aside(&data, exit(), "a/b/CLAUDE.md"));
    assert_eq!(moved[0].within, ["a", "b"]);
    assert_eq!(moved[0].reason, Reason::New);
    assert!(!root.join("a/b/CLAUDE.md").exists());
    assert!(root.join("a/b/brief.md").is_file());
}

/// `PR4a`: nested instruction files there at launch and only edited in place keep their file
/// IDs, so they stay where they are.
#[test]
fn nested_instruction_files_there_at_launch_and_unchanged_stay() {
    let (w, root, data) = world("aside-nested-claude-kept");
    w.dir.file("matter/sub/CLAUDE.md", "# the lawyer's\n");
    w.dir.file("matter/sub/AGENTS.md", "# the lawyer's\n");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    assert!(record(&data).names.contains_key(&key("sub/CLAUDE.md")));
    w.dir.file("matter/sub/CLAUDE.md", "# edited in place\n");

    assert_eq!(
        exit_as(&volume(), &root, &data, "s1", exit()).unwrap(),
        none()
    );
    assert!(root.join("sub/CLAUDE.md").is_file());
    assert!(root.join("sub/AGENTS.md").is_file());
}

/// `PR4a`: a nested `AGENTS.md` swapped for another file (another file ID) is set aside as
/// changed, and one renamed in where none was is set aside as new.
#[test]
fn a_nested_agents_md_replaced_or_renamed_in_is_set_aside() {
    let (w, root, data) = world("aside-nested-agents-md");
    w.dir.file("matter/sub/AGENTS.md", "# the lawyer's\n");
    let swapped = w.dir.file("matter/drafts/swap.md", "# the agent's\n");
    let renamed = w.dir.file("matter/drafts/rename.md", "# the agent's\n");
    w.dir.dir("matter/other");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();

    rename_new(&root.join("sub/AGENTS.md"), &root.join("sub/kept.md")).unwrap();
    rename_new(&swapped, &root.join("sub/AGENTS.md")).unwrap();
    rename_new(&renamed, &root.join("other/AGENTS.md")).unwrap();
    let moved = exit_as(&volume(), &root, &data, "s1", exit()).unwrap();

    let mut seen: Vec<_> = moved.iter().map(|e| (e.from.clone(), e.reason)).collect();
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        seen,
        [
            (root.join("other/AGENTS.md"), Reason::New),
            (root.join("sub/AGENTS.md"), Reason::Changed),
        ]
    );
    assert!(root.join("sub/kept.md").is_file());
}

/// `PR4a`: names match as NTFS opens them, case ignored: `claude.md` and `Agents.MD` are the
/// probes' `CLAUDE.md` and `AGENTS.md`, each set aside under the name probed.
#[test]
fn nested_instruction_files_match_in_any_case() {
    let (w, root, data) = world("aside-nested-case");
    w.dir.dir("matter/sub/deep");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    w.dir.file("matter/sub/claude.md", "# the agent's\n");
    w.dir.file("matter/sub/deep/Agents.MD", "# the agent's\n");

    let moved = exit_as(&volume(), &root, &data, "s1", exit()).unwrap();

    let mut from: Vec<_> = moved.iter().map(|e| e.from.clone()).collect();
    from.sort();
    let expect = [root.join("sub/CLAUDE.md"), root.join("sub/deep/AGENTS.md")];
    assert_eq!(from, expect);
    assert!(moved.iter().all(|e| e.reason == Reason::New));
    let left = std::fs::read_dir(root.join("sub/deep")).unwrap().count();
    assert_eq!(left, 0, "Agents.MD moved");
    assert!(!root.join("sub/claude.md").exists());
}

/// `PR4a`: `CLAUDE.local.md` is checked only at the root (D-B17 left it out below, for the
/// scan's cost); a nested one stays.
#[test]
fn a_nested_claude_local_md_is_not_probed() {
    let (w, root, data) = world("aside-nested-local");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    w.dir.file("matter/sub/claude.local.md", "# the agent's\n");

    assert_eq!(
        exit_as(&volume(), &root, &data, "s1", exit()).unwrap(),
        none()
    );
    assert!(root.join("sub/claude.local.md").is_file());
}
