//! R2: every link in the matter tree (a junction on NTFS, a symbolic link in the stand-in),
//! recorded by its own file ID whatever its name. One new or changed in a session is set aside
//! as itself, never followed; a lawyer's link there at launch and unchanged stays.

use std::path::Path;

use super::{exit, exit_as, key, launch, launch_as, none, record, volume, world};
use crate::windows::set_aside::Reason;
use crate::windows::test_world::World;
use crate::windows::write::rename_new;

/// Makes `rel` in `w` a link to the folder `target`: a junction on Windows, a symbolic link
/// elsewhere.
fn link(w: &World, rel: &str, target: &Path) {
    #[cfg(windows)]
    crate::windows::test_junction::junction(&w.at(rel), target).unwrap();
    #[cfg(unix)]
    w.dir.symlink(rel, target);
}

/// Whether something is at `path` itself, a link included.
fn there(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// R2: the agent moves the lawyer's `matter\sub` out of the matter, renames a lawyer folder
/// inside it to `.git`, and makes `matter\sub` a link to it. Git in `matter\sub` would use that
/// `.git`, which no probe in the matter sees. The after-session pass sets the link aside as
/// itself and leaves what it names alone.
#[test]
fn a_link_the_agent_makes_to_a_folder_holding_git_is_set_aside_as_itself() {
    let (w, root, data) = world("aside-link-sub");
    w.dir
        .file("matter/sub/drafts/hooks/post-checkout", "#!/bin/sh\n");
    w.dir.dir("elsewhere");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();

    let moved_out = w.canonical("elsewhere").join("moved");
    rename_new(&root.join("sub"), &moved_out).unwrap();
    rename_new(&moved_out.join("drafts"), &moved_out.join(".git")).unwrap();
    link(&w, "matter/sub", &moved_out);
    let moved = exit_as(&volume(), &root, &data, "s1", exit()).unwrap();

    let seen: Vec<_> = moved.iter().map(|e| (e.from.clone(), e.reason)).collect();
    assert_eq!(seen, [(root.join("sub"), Reason::New)]);
    assert!(!there(&root.join("sub")));
    assert!(there(&moved[0].to), "the link itself was moved");
    assert!(
        moved_out.join(".git/hooks/post-checkout").is_file(),
        "never followed"
    );
    assert!(record(&data).links.is_empty());
}

/// R2: a lawyer's link there at launch and unchanged stays, and stays recorded.
#[test]
fn a_lawyers_link_unchanged_since_launch_stays() {
    let (w, root, data) = world("aside-link-kept");
    w.dir.dir("elsewhere/precedents/.git");
    link(&w, "matter/Docs", &w.canonical("elsewhere/precedents"));
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    assert!(record(&data).links.contains_key(&key("Docs")));

    assert_eq!(
        exit_as(&volume(), &root, &data, "s1", exit()).unwrap(),
        none()
    );
    assert!(there(&root.join("Docs")));
    assert!(record(&data).links.contains_key(&key("Docs")));
}

/// R2: a lawyer's link swapped in a session for another link of the same name is set aside as
/// changed; the old link under its new name is new there, and goes too.
#[test]
fn a_link_swapped_since_launch_is_set_aside_as_changed() {
    let (w, root, data) = world("aside-link-swapped");
    w.dir.dir("elsewhere/precedents");
    w.dir.dir("elsewhere/planted/.git");
    w.dir.dir("matter/deep");
    link(&w, "matter/deep/Docs", &w.canonical("elsewhere/precedents"));
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();

    rename_new(&root.join("deep/Docs"), &root.join("deep/old")).unwrap();
    link(&w, "matter/deep/Docs", &w.canonical("elsewhere/planted"));
    let moved = exit_as(&volume(), &root, &data, "s1", exit()).unwrap();

    let mut seen: Vec<_> = moved.iter().map(|e| (e.from.clone(), e.reason)).collect();
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        seen,
        [
            (root.join("deep/Docs"), Reason::Changed),
            (root.join("deep/old"), Reason::New),
        ]
    );
}
