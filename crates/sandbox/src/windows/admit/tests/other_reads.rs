//! The gate's listing for the reads of other matters' folders (R18): which allows each read
//! needs, none reaching a refused name.

use std::path::{Path, PathBuf};

use super::*;
use crate::windows::acl_plan::{Inherit, Rights, plan};
use crate::windows::test_world::{FakeFolders, World, matter_one, sids};

/// The reads the gate lists for `policy`, each relative to matter two's folder.
fn listed(w: &World, policy: &Policy) -> Vec<(String, Inherit)> {
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let data = w.at("data");
    let others = w.neighbour_m2();
    let admitted = admit_with(policy, &data, &others, ok, &FakeFolders::default()).unwrap();
    let root = w.canonical("other/m2");
    let relative = |path: &Path| {
        let rest = path.strip_prefix(&root).unwrap();
        rest.to_string_lossy().replace('\\', "/")
    };
    let reads = admitted.other_reads().iter();
    reads.map(|r| (relative(r.path()), r.inherit())).collect()
}

fn this_only(rel: &str) -> (String, Inherit) {
    (rel.to_owned(), Inherit::ThisOnly)
}

fn tree(rel: &str) -> (String, Inherit) {
    (rel.to_owned(), Inherit::Tree)
}

#[test]
fn a_folder_with_no_refused_name_in_it_takes_one_allow_for_its_tree() {
    let w = World::new("reads-clean");
    w.other_tree();
    let policy = w.reading(&[("other/m2/docs", false)]);
    assert_eq!(listed(&w, &policy), vec![tree("docs")]);
}

#[test]
fn a_folder_holding_refused_names_at_any_depth_leaves_each_out() {
    let w = World::new("reads-refused");
    w.other_tree();
    w.dir.file("other/m2/sub/src/.AGENTS/x.md", "x");
    let policy = w.reading(&[("other/m2", false)]);
    // `.git`, `.Claude`, `sub/.git`, `sub/src/.AGENTS` and `wt/.git` (a worktree's file) get
    // nothing; every folder above one gets an allow for itself alone, and what is beside one
    // an allow for its tree, or for itself when a file.
    let expected = vec![
        this_only(""),
        tree("docs"),
        this_only("sub"),
        this_only("sub/src"),
        this_only("sub/src/d.rs"),
        this_only("sub/e.txt"),
        this_only("wt"),
        this_only("wt/f.txt"),
        this_only("a.txt"),
    ];
    assert_eq!(listed(&w, &policy), expected);
}

#[cfg(unix)]
#[test]
fn a_link_or_a_file_with_another_name_is_left_out() {
    let w = World::new("reads-links");
    w.other_tree();
    let outside = w.dir.file("elsewhere/secret.txt", "s");
    w.dir.symlink("other/m2/link.txt", &outside);
    w.dir.symlink("other/m2/docs-link", &w.at("elsewhere"));
    w.dir.hard_link("other/m2/hard.txt", &outside);
    let policy = w.reading(&[("other/m2", false)]);
    let reads = listed(&w, &policy);
    for left_out in ["link.txt", "docs-link", "hard.txt"] {
        assert!(reads.iter().all(|(rel, _)| rel != left_out), "{left_out}");
    }
    // A granted file with another name gets nothing either.
    let policy = w.reading(&[("other/m2/hard.txt", true)]);
    assert_eq!(listed(&w, &policy), Vec::new());
}

#[test]
fn a_granted_file_takes_one_allow_for_itself_and_reads_are_listed_once() {
    let w = World::new("reads-file");
    w.other_tree();
    let reads = [
        ("other/m2/sub/e.txt", true),
        ("other/m2/sub", false),
        ("other/m2/sub/e.txt", true),
    ];
    let policy = w.reading(&reads);
    let expected = vec![this_only("sub/e.txt"), this_only("sub"), tree("sub/src")];
    assert_eq!(listed(&w, &policy), expected);
}

#[test]
fn a_granted_folder_that_cant_take_entries_refuses_the_launch() {
    let w = World::new("reads-network");
    w.other_tree();
    let policy = w.reading(&[("other/m2/docs", false)]);
    let docs = w.canonical("other/m2/docs");
    let check = answering(docs, || Ok(FolderSupport::Network));
    let data = w.at("data");
    let others = w.neighbour_m2();
    let error = admit_with(&policy, &data, &others, check, &FakeFolders::default()).unwrap_err();
    assert!(
        matches!(
            error,
            Error::FolderUnsupported {
                path_kind: PathKind::OtherMatterRead,
                support: FolderRefusal::Support(FolderSupport::Network),
            }
        ),
        "{error:?}"
    );
}

#[test]
fn the_plan_allows_reads_for_the_pool_user_only_and_denies_nothing_for_them() {
    let w = World::new("reads-plan");
    let root = w.other_tree();
    let policy = w.reading(&[("other/m2", false)]);
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let (data, others) = (w.at("data"), w.neighbour_m2());
    let admitted = admit_with(&policy, &data, &others, ok, &FakeFolders::default()).unwrap();
    let plan = plan(&admitted, &sids(), matter_one()).unwrap();
    let reads: Vec<_> = plan
        .grants()
        .iter()
        .filter(|g| g.kind() == PathKind::OtherMatterRead)
        .collect();
    assert_eq!(reads.len(), admitted.other_reads().len());
    for grant in &reads {
        assert_eq!(grant.rights(), Rights::ReadAndExecute);
        assert_eq!(grant.sid(), &sids().pool_user);
    }
    // Nothing at all is planned in a refused name, and the only entries in matter two's
    // folder besides the reads are its fence.
    let refused: Vec<PathBuf> = [".git", ".Claude", "sub/.git", "wt/.git"]
        .iter()
        .map(|rel| root.join(rel))
        .collect();
    for grant in plan.grants() {
        assert!(
            refused.iter().all(|r| !grant.path().starts_with(r)),
            "{grant:?}"
        );
        if grant.path().starts_with(&root) && grant.kind() != PathKind::OtherMatterRead {
            assert_eq!(grant.path(), root, "{grant:?}");
            assert_eq!(grant.rights(), Rights::DenyReadWrite, "{grant:?}");
        }
    }
}
