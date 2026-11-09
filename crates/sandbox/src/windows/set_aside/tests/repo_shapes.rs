//! The after-session pass over a repository's other shapes: a `HEAD` file outside every `.git`
//! (a bare repository's, in a lawyer's folder git would trust) and a `.git` only the HFS+ strip
//! names, both taken from the folders' listings; and a record an older version wrote, which
//! holds no `HEAD` and so takes none for new.

use std::ffi::OsString;

use super::{after, before, exit, launch, record, world};
use crate::windows::set_aside::Reason;
use crate::windows::set_aside::files::{HEADS_RECORDED, Place, Record};
use crate::windows::test_world::matter_one;

/// Makes `folder` (below the matter) a bare repository's layout.
fn bare(w: &crate::windows::test_world::World, folder: &str) {
    w.dir
        .file(&format!("matter/{folder}/objects/info/packs"), "");
    w.dir
        .file(&format!("matter/{folder}/refs/heads/main"), "0123\n");
    w.dir
        .file(&format!("matter/{folder}/HEAD"), "ref: refs/heads/main\n");
}

#[test]
fn a_head_made_in_a_lawyers_folder_is_set_aside_after_the_session() {
    let (w, root, data) = world("aside-head");
    w.dir.file("matter/Precedents/brief.md", "# the lawyer's\n");
    before(&root, &data, launch()).unwrap();

    bare(&w, "Precedents");
    bare(&w, "new/repo");
    let moved = after(&root, &data, exit()).unwrap();

    let mut seen: Vec<_> = moved.iter().map(|e| e.from.clone()).collect();
    seen.sort();
    assert_eq!(
        seen,
        [root.join("Precedents/HEAD"), root.join("new/repo/HEAD")]
    );
    assert!(moved.iter().all(|e| e.reason == Reason::New));
    assert!(root.join("Precedents/brief.md").is_file());
    assert!(
        root.join("Precedents/objects").is_dir(),
        "the lawyer's folder stays"
    );
}

#[test]
fn a_head_inside_a_git_and_a_lawyers_bare_repository_unchanged_stay() {
    let (w, root, data) = world("aside-head-kept");
    w.dir.file("matter/.git/HEAD", "ref: refs/heads/main\n");
    bare(&w, "archive/old.git-free");
    before(&root, &data, launch()).unwrap();

    // The lawyer's own git moves its branch: a new `.git\HEAD`.
    crate::windows::write::write_atomic(&root.join(".git/HEAD"), b"ref: refs/heads/other\n")
        .unwrap();
    let moved = after(&root, &data, exit()).unwrap();

    assert_eq!(moved, Vec::new());
    assert!(root.join("archive/old.git-free/HEAD").is_file());
    let key = vec![
        OsString::from("archive"),
        "old.git-free".into(),
        "HEAD".into(),
    ];
    assert!(
        record(&data).names.contains_key(&key),
        "recorded by its file ID"
    );
}

#[test]
fn a_git_named_with_an_hfs_ignorable_is_set_aside_after_the_session() {
    let (w, root, data) = world("aside-hfs-git");
    w.dir.file("matter/Forms/hooks/pre-commit", "#!/bin/sh\n");
    w.dir.file("matter/sub.g/keep.txt", "");
    before(&root, &data, launch()).unwrap();

    let lookalike = root.join("sub.g/.g\u{200C}it");
    crate::windows::write::rename_new(&root.join("Forms"), &lookalike).unwrap();
    let moved = after(&root, &data, exit()).unwrap();

    let seen: Vec<_> = moved.iter().map(|e| e.from.clone()).collect();
    assert_eq!(seen, [lookalike]);
    assert!(moved[0].to.join("hooks/pre-commit").is_file());
}

/// A record an older version wrote holds no `HEAD`: its pass records them and moves none, and
/// the next record holds them.
#[test]
fn a_record_without_heads_takes_no_head_for_new() {
    let (w, root, data) = world("aside-head-old-record");
    bare(&w, "Precedents");
    before(&root, &data, launch()).unwrap();
    let place = Place::new(&data, &matter_one());
    let old = record(&data);
    let mut names = old.names;
    names.retain(|key, _| key.last().is_some_and(|name| name != "HEAD"));
    names.remove(&vec![OsString::from(HEADS_RECORDED)]);
    let seen = crate::windows::set_aside::files::Snapshot {
        names,
        links: old.links,
    };
    {
        let _held = place.lock().unwrap();
        place
            .write_record(&Record::new(matter_one(), old.sessions, seen))
            .unwrap();
    }
    assert!(!record(&data).holds_heads());

    let moved = after(&root, &data, exit()).unwrap();

    assert_eq!(moved, Vec::new());
    assert!(root.join("Precedents/HEAD").is_file());
    assert!(record(&data).holds_heads());
}

/// The widened shape after the session: a `HEAD` that is a link, and a `HEAD` beside
/// `commondir`, each made in a lawyer's folder; the `HEAD` goes and the folder stays.
#[test]
fn a_linked_head_and_a_commondir_head_are_set_aside_after_the_session() {
    let (w, root, data) = world("aside-head-shapes");
    w.dir.file("matter/A/brief.md", "# the lawyer's\n");
    w.dir.file("matter/B/brief.md", "# the lawyer's\n");
    before(&root, &data, launch()).unwrap();

    w.dir.file("matter/A/objects/info/packs", "");
    w.dir.file("matter/A/refs/heads/main", "0123\n");
    super::links::link(&w, "matter/A/HEAD", &root.join("B"));
    w.dir.file("matter/B/commondir", "../elsewhere/.git\n");
    w.dir.file("matter/B/HEAD", "0123\n");
    let moved = after(&root, &data, exit()).unwrap();

    let mut seen: Vec<_> = moved.iter().map(|e| e.from.clone()).collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen, [root.join("A/HEAD"), root.join("B/HEAD")]);
    assert!(root.join("A/brief.md").is_file() && root.join("B/commondir").is_file());
}

/// M1, on #784's deny: edits inside a git folder that was there at launch raise no event the
/// watch acts on (it keeps a change record only for a reparse point), because the Windows plan
/// write-denies each such folder's whole tree (`acl_plan` test
/// `a_bare_repository_in_the_matter_gets_the_git_folders_deny`, from `MatterScan`'s
/// `bare_repos` and `git_dirs`). So every git folder the set-aside's launch scan knows (each
/// folder holding a `HEAD` outside every `.git`, each git folder's layout, each folder a
/// `commondir` names) must be one of those.
#[test]
fn every_git_folder_the_watch_knows_at_launch_is_one_the_plan_denies() {
    let (w, root, _data) = world("aside-m1");
    bare(&w, "precedents.git");
    w.dir.file("matter/linked/HEAD", "ref: refs/heads/other\n");
    w.dir.file("matter/linked/commondir", "../common\n");
    w.dir.dir("matter/common");
    w.dir.file("matter/lone/HEAD", "ref: refs/heads/main\n");
    w.dir
        .file("matter/deep/er/x.git/HEAD", "ref: refs/heads/main\n");

    let found = super::super::scan::scan(&super::volume(), &root);
    let path = |key: &[OsString]| key.iter().fold(root.clone(), |at, part| at.join(part));
    let mut known: Vec<std::path::PathBuf> = found
        .items
        .iter()
        .filter(|item| item.kept == super::super::scan::Kept::Head)
        .map(|item| path(&item.within))
        .chain(found.bare.iter().map(|(key, _)| path(key)))
        .chain(found.common.iter().map(|key| path(key)))
        .collect();
    known.sort();
    known.dedup();
    assert_eq!(known.len(), 5, "{known:?}");

    let scan = crate::links::scan_matter(&root).unwrap();
    let denied: Vec<&std::path::PathBuf> =
        scan.bare_repos().iter().chain(scan.git_dirs()).collect();
    for folder in &known {
        assert!(
            denied.contains(&folder),
            "{folder:?} isn't denied: {denied:?}"
        );
    }
}
