//! The restore's refusals besides a git folder (`run/refusals.rs`): what was set aside from a
//! folder a launch `commondir` names, and a link leading out of the matter.

use super::links::link;
use super::{after, before, exit, launch, volume, world};
use crate::windows::set_aside::{SetAsideFailure, run};

const HEAD: &str = "ref: refs/heads/main\n";

/// A `config`, `hooks` or `objects` made where a launch `commondir` points is set aside after
/// the session, and never put back: git would read it as the lawyer's.
#[test]
fn restore_refuses_what_came_from_a_launch_commondir_folder() {
    let (w, root, data) = world("restore-common");
    w.dir.file("matter/wt/HEAD", HEAD);
    w.dir.file("matter/wt/commondir", "..\n");
    before(&root, &data, launch()).unwrap();

    w.dir.file("matter/config", "[core]\n\tfsmonitor = evil\n");
    w.dir.file("matter/hooks/post-checkout", "#!/bin/sh\n");
    w.dir.file("matter/objects/info/packs", "");
    let moved = after(&root, &data, exit()).unwrap();
    assert_eq!(moved.len(), 3, "{moved:?}");

    for entry in &moved {
        let refused = run::restore(&volume(), &data, entry).unwrap_err();
        assert_eq!(
            refused.failure,
            SetAsideFailure::GitFolder,
            "{:?}",
            entry.from
        );
        assert!(entry.to.exists(), "left where it is");
    }
}

/// A link made in the session is set aside after it; one whose target leads outside the matter
/// is never put back, one leading inside it comes back.
#[test]
fn restore_refuses_a_link_leading_out_of_the_matter() {
    let (w, root, data) = world("restore-link-out");
    w.dir.dir("matter/inside");
    w.dir.dir("elsewhere");
    before(&root, &data, launch()).unwrap();

    link(&w, "matter/out", &w.canonical("elsewhere"));
    link(&w, "matter/sub-in", &root.join("inside"));
    let mut moved = after(&root, &data, exit()).unwrap();
    moved.sort_by(|a, b| a.from.cmp(&b.from));
    let [out, within] = moved.as_slice() else {
        panic!("two links: {moved:?}");
    };
    assert_eq!(out.from, root.join("out"));

    let refused = run::restore(&volume(), &data, out).unwrap_err();
    assert_eq!(refused.failure, SetAsideFailure::LinkOut);
    assert!(
        std::fs::symlink_metadata(&out.to).is_ok(),
        "left where it is"
    );

    run::restore(&volume(), &data, within).unwrap();
    assert!(
        std::fs::symlink_metadata(root.join("sub-in")).is_ok(),
        "put back"
    );
}
