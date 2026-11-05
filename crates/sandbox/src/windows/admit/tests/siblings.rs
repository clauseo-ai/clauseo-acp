//! The gate's walk of the matter's parent: which folders get the group's deny.

use super::*;

/// The world's matter with a registered matter and two plain folders beside it, listed for
/// real, and the canonical paths of the plain ones and the registered one.
fn with_siblings(w: &World) -> (Vec<PathBuf>, PathBuf) {
    for rel in ["Drafts", "Precedents", "registered"] {
        w.dir.dir(rel);
    }
    let plain = vec![w.canonical("Drafts"), w.canonical("Precedents")];
    (plain, w.canonical("registered"))
}

#[test]
fn folders_beside_the_matter_are_siblings_but_not_the_matter_or_a_registered_one() {
    // The real listing (`ReadDir`) of the matter's parent: the matter, the data folder, the home
    // and the registered matter are left out, and each plain folder is admitted as a sibling.
    let w = World::new("admit-siblings");
    let (plain, reg_root) = with_siblings(&w);
    let policy = w.builder().build().unwrap();
    let check = |_: &Path| Ok(FolderSupport::Ok);
    let reg = [reg_root.clone()];
    let neighbours = registered(&reg);
    let admitted = admit_with(&policy, &w.at("data"), &neighbours, check, &ReadDir).unwrap();
    let siblings = admitted.siblings();
    for path in &plain {
        assert!(siblings.contains(path), "{path:?} in {siblings:?}");
    }
    let excluded = [
        w.canonical("matter"),
        w.canonical("data"),
        w.canonical("home"),
        reg_root,
    ];
    for path in &excluded {
        assert!(!siblings.contains(path), "{path:?} in {siblings:?}");
    }
}

#[test]
fn a_sibling_that_cant_take_the_deny_refuses_the_launch() {
    let w = World::new("admit-sibling-refused");
    let (plain, reg_root) = with_siblings(&w);
    let policy = w.builder().build().unwrap();
    let (mut ledger, before) = ledger_with_an_entry(&w);
    let listing = FakeFolders {
        listing: plain.clone(),
        ..FakeFolders::default()
    };
    let check = answering(plain[0].clone(), || {
        Ok(FolderSupport::CantChangePermissions)
    });
    let refused = prepare(
        &policy,
        &w.at("data"),
        &[reg_root],
        check,
        &listing,
        &mut ledger,
    );
    let Err(Error::FolderUnsupported { path_kind, support }) = refused else {
        panic!("not refused: {refused:?}");
    };
    let want = FolderRefusal::Support(FolderSupport::CantChangePermissions);
    assert_eq!((path_kind, support), (PathKind::Sibling, want));
    assert_eq!(std::fs::read(ledger.path()).unwrap(), before);
}

#[test]
fn a_listing_that_fails_refuses_and_a_share_or_missing_sibling_is_left_alone() {
    let w = World::new("admit-sibling-listing");
    let (plain, _) = with_siblings(&w);
    let policy = w.builder().build().unwrap();
    let check = |_: &Path| Ok(FolderSupport::Ok);
    let failing = FakeFolders {
        failing: true,
        ..FakeFolders::default()
    };
    let refused = admit_with(
        &policy,
        &w.at("data"),
        &Neighbours::default(),
        check,
        &failing,
    );
    let Err(Error::FolderUnsupported { path_kind, .. }) = refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(path_kind, PathKind::Sibling);
    let mut listing = plain.clone();
    listing.push(w.at("gone"));
    let listed = FakeFolders {
        listing,
        ..FakeFolders::default()
    };
    let share = answering(plain[0].clone(), || Ok(FolderSupport::Network));
    let admitted = admit_with(
        &policy,
        &w.at("data"),
        &Neighbours::default(),
        share,
        &listed,
    )
    .unwrap();
    assert_eq!(admitted.siblings(), &plain[1..]);
}

#[test]
fn a_sibling_holding_a_registered_matter_is_walked_down_to_it() {
    // `other` holds the registered `other/m2`: `other` and `m2` get no deny, `other/m3` beside
    // `m2` does. A folder holding one of this policy's read paths is left alone whole.
    let w = World::new("admit-sibling-holds");
    w.dir.dir("Tools/bin");
    let policy = w.builder().read(w.at("Tools/bin")).build().unwrap();
    let check = |_: &Path| Ok(FolderSupport::Ok);
    let neighbours = registered(&[w.canonical("other/m2")]);
    let admitted = admit_with(&policy, &w.at("data"), &neighbours, check, &ReadDir).unwrap();
    let siblings = admitted.siblings();
    assert!(siblings.contains(&w.canonical("other/m3")), "{siblings:?}");
    for path in ["other", "other/m2", "Tools", "Tools/bin"] {
        assert!(
            !siblings.contains(&w.canonical(path)),
            "{path} in {siblings:?}"
        );
    }
}

#[test]
fn nothing_another_matters_policy_reaches_gets_a_deny() {
    // Matter 2's allowed folder sits beside matter 1: a group deny there would block matter 2.
    let w = World::new("admit-sibling-reach");
    w.dir.dir("Shared/Precedents");
    w.dir.dir("Shared/Other");
    let policy = w.builder().build().unwrap();
    let check = |_: &Path| Ok(FolderSupport::Ok);
    let neighbours = Neighbours {
        reach: vec![w.canonical("Shared/Precedents"), w.canonical("elsewhere")],
        ..Neighbours::default()
    };
    let admitted = admit_with(&policy, &w.at("data"), &neighbours, check, &ReadDir).unwrap();
    let siblings = admitted.siblings();
    for path in [
        "Shared",
        "Shared/Precedents",
        "elsewhere",
        "elsewhere/Precedents",
    ] {
        assert!(
            !siblings.contains(&w.canonical(path)),
            "{path} in {siblings:?}"
        );
    }
    assert!(
        siblings.contains(&w.canonical("Shared/Other")),
        "{siblings:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_linked_sibling_is_left_alone() {
    let w = World::new("admit-sibling-link");
    let target = w.dir.dir("elsewhere/target");
    let link = w.dir.symlink("linked", &target);
    let policy = w.builder().build().unwrap();
    let listed = FakeFolders {
        listing: vec![link],
        ..FakeFolders::default()
    };
    let check = |_: &Path| Ok(FolderSupport::Ok);
    let admitted = admit_with(
        &policy,
        &w.at("data"),
        &Neighbours::default(),
        check,
        &listed,
    )
    .unwrap();
    assert_eq!(admitted.siblings(), [] as [PathBuf; 0]);
}
