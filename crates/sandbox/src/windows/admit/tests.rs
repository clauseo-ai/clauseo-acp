use std::io;
use std::path::{Path, PathBuf};

use super::*;
use crate::windows::acl_plan::{AdmittedPlan, plan};
use crate::windows::ledger::{Ledger, ledger_path};
use crate::windows::test_world::{
    FakeFolders, NOTES, STATE, STORE, World, matter_one, matter_two, reserved, sids, trusted,
};

/// A folder check that answers `answer` for `folder` and `Ok` for every other path.
fn answering(
    folder: PathBuf,
    answer: fn() -> io::Result<FolderSupport>,
) -> impl FnMut(&Path) -> io::Result<FolderSupport> {
    move |path| {
        if path == folder {
            answer()
        } else {
            Ok(FolderSupport::Ok)
        }
    }
}

/// The steps a launch takes up to the first permission entry: the gate, the plan, the ledger's
/// pending entries. A refusal returns at the gate.
fn prepare(
    policy: &Policy,
    data: &Path,
    others: &[PathBuf],
    check: impl FnMut(&Path) -> io::Result<FolderSupport>,
    groups: &FakeFolders,
    ledger: &mut Ledger,
) -> Result<usize, Error> {
    let admitted = admit_with(policy, data, others, check, groups)?;
    let mut plan = plan(&admitted, &sids(), matter_one())?;
    ledger.record_pending(&mut plan).map_err(Error::Probe)?;
    Ok(plan.entries().len())
}

/// A ledger with one of another matter's entries already in it, and its bytes.
fn ledger_with_an_entry(w: &World) -> (Ledger, Vec<u8>) {
    let path = ledger_path(&w.at("data"));
    let mut ledger = Ledger::open(&path, &trusted(), &reserved()).unwrap();
    let other = w.builder().build().unwrap();
    let admitted = admit_with(
        &other,
        &w.at("data"),
        &[],
        |_| Ok(FolderSupport::Ok),
        &FakeFolders::default(),
    );
    let grants = plan(&admitted.unwrap(), &sids(), matter_one()).unwrap();
    let mut two = AdmittedPlan::from_grants(matter_two(), grants.grants()[..2].to_vec());
    ledger.record_pending(&mut two).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    (ledger, bytes)
}

#[test]
fn every_grant_folder_is_checked() {
    let w = World::new("admit-checks-all");
    let policy = w.full_policy();
    let mut asked = Vec::new();
    let check = |path: &Path| {
        asked.push(path.to_owned());
        Ok(FolderSupport::Ok)
    };
    admit_with(&policy, &w.at("data"), &[], check, &FakeFolders::default()).unwrap();
    let want = [
        w.canonical("matter"),
        w.canonical(STATE),
        w.canonical(NOTES),
        w.canonical(STORE),
    ];
    assert_eq!(asked, want);
}

#[test]
fn each_refused_folder_names_its_kind_and_not_its_path() {
    let w = World::new("admit-kinds");
    let policy = w.full_policy();
    let root = dunce::canonicalize(w.dir.path()).unwrap();
    let cases = [
        ("matter", PathKind::Matter),
        (STATE, PathKind::State),
        (NOTES, PathKind::Notes),
        (STORE, PathKind::ClaudeStore),
    ];
    let answers: [fn() -> io::Result<FolderSupport>; 4] = [
        || Ok(FolderSupport::CantChangePermissions),
        || Ok(FolderSupport::Network),
        || Ok(FolderSupport::NoPermissions),
        || {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "C:\\m\\Client v Other is missing",
            ))
        },
    ];
    for (rel, kind) in cases {
        for answer in answers {
            let check = answering(w.canonical(rel), answer);
            let error = admit_with(&policy, &w.at("data"), &[], check, &FakeFolders::default())
                .unwrap_err();
            let Error::FolderUnsupported { path_kind, support } = &error else {
                panic!("{error:?}");
            };
            assert_eq!(*path_kind, kind);
            let want = match answer() {
                Ok(support) => FolderRefusal::Support(support),
                Err(error) => FolderRefusal::CheckFailed(error.kind()),
            };
            assert_eq!(*support, want);
            let text = format!("{error} {error:?}");
            assert!(!text.contains(&*root.to_string_lossy()), "a path in {text}");
            assert!(
                !text.contains("Client v Other"),
                "the io error's text in {text}"
            );
        }
    }
}

#[test]
fn a_refusal_leaves_the_ledger_byte_identical() {
    let w = World::new("admit-ledger-unchanged");
    let policy = w.full_policy();
    let (mut ledger, before) = ledger_with_an_entry(&w);
    let path = ledger.path().to_owned();
    for rel in ["matter", STATE, NOTES] {
        let check = answering(w.canonical(rel), || Ok(FolderSupport::Network));
        let refused = prepare(
            &policy,
            &w.at("data"),
            &[],
            check,
            &FakeFolders::default(),
            &mut ledger,
        );
        assert!(matches!(refused, Err(Error::FolderUnsupported { .. })));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "after a refusal on {rel}"
        );
    }
    // The same launch, admitted, does write: the refusal is what kept the file as it was.
    let check = |_: &Path| Ok(FolderSupport::Ok);
    let planned = prepare(
        &policy,
        &w.at("data"),
        &[],
        check,
        &FakeFolders::default(),
        &mut ledger,
    )
    .unwrap();
    assert!(planned > 0);
    assert_ne!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn a_refusal_on_a_fresh_data_folder_makes_no_ledger() {
    let w = World::new("admit-no-ledger");
    let policy = w.full_policy();
    let path = ledger_path(&w.at("data"));
    let mut ledger = Ledger::open(&path, &trusted(), &reserved()).unwrap();
    let check = answering(w.canonical("matter"), || Ok(FolderSupport::NoPermissions));
    assert!(
        prepare(
            &policy,
            &w.at("data"),
            &[],
            check,
            &FakeFolders::default(),
            &mut ledger
        )
        .is_err()
    );
    assert!(!path.exists());
    assert!(!w.at("data/sandbox").exists());
}

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
    let (plain, registered) = with_siblings(&w);
    let policy = w.builder().build().unwrap();
    let check = |_: &Path| Ok(FolderSupport::Ok);
    let reg = [registered.clone()];
    let admitted = admit_with(&policy, &w.at("data"), &reg, check, &ReadDir).unwrap();
    let siblings = admitted.siblings();
    for path in &plain {
        assert!(siblings.contains(path), "{path:?} in {siblings:?}");
    }
    let excluded = [
        w.canonical("matter"),
        w.canonical("data"),
        w.canonical("home"),
        registered,
    ];
    for path in &excluded {
        assert!(!siblings.contains(path), "{path:?} in {siblings:?}");
    }
}

#[test]
fn a_sibling_that_cant_take_the_deny_refuses_the_launch() {
    let w = World::new("admit-sibling-refused");
    let (plain, registered) = with_siblings(&w);
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
        &[registered],
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
    let refused = admit_with(&policy, &w.at("data"), &[], check, &failing);
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
    let admitted = admit_with(&policy, &w.at("data"), &[], share, &listed).unwrap();
    assert_eq!(admitted.siblings(), &plain[1..]);
}

#[test]
fn a_sibling_holding_a_registered_matter_or_a_policy_path_is_left_alone() {
    // A deny there would reach the registered matter's own pool user, or the jail's own reads.
    let w = World::new("admit-sibling-holds");
    w.dir.dir("Clients/registered");
    w.dir.dir("Tools/bin");
    let policy = w.builder().read(w.at("Tools/bin")).build().unwrap();
    let listed = FakeFolders {
        listing: vec![w.canonical("Clients"), w.canonical("Tools")],
        ..FakeFolders::default()
    };
    let check = |_: &Path| Ok(FolderSupport::Ok);
    let reg = [w.canonical("Clients/registered")];
    let admitted = admit_with(&policy, &w.at("data"), &reg, check, &listed).unwrap();
    assert_eq!(admitted.siblings(), [] as [PathBuf; 0]);
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
    let admitted = admit_with(&policy, &w.at("data"), &[], check, &listed).unwrap();
    assert_eq!(admitted.siblings(), [] as [PathBuf; 0]);
}

#[test]
fn another_matter_nested_with_this_one_is_refused_whatever_its_dacl() {
    // Matter `m` and another matter `m\B` inside it, then the other way round. The folder
    // checks all say Ok.
    let w = World::new("admit-nested");
    w.dir.dir("matter/B");
    let policy = w.builder().build().unwrap();
    let (mut ledger, before) = ledger_with_an_entry(&w);
    let inner = w.canonical("matter/B");
    let groups = FakeFolders::default();
    let check = |_: &Path| Ok(FolderSupport::Ok);
    let refused = prepare(
        &policy,
        &w.at("data"),
        &[inner],
        check,
        &groups,
        &mut ledger,
    );
    let Err(Error::FolderUnsupported { path_kind, support }) = refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(
        (path_kind, support),
        (PathKind::OtherMatter, FolderRefusal::NestedMatter)
    );
    assert!(
        groups.asked.borrow().is_empty(),
        "refused before the siblings are listed"
    );
    assert_eq!(std::fs::read(ledger.path()).unwrap(), before);

    let inside = Policy::builder(w.at("matter/B"), w.at("elsewhere"))
        .home(w.at("home"))
        .build()
        .unwrap();
    let outer = [w.at("matter")];
    let refused = admit_with(&inside, &w.at("data"), &outer, check, &groups);
    let Err(Error::FolderUnsupported { support, .. }) = refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(support, FolderRefusal::NestedMatter);
}

/// Asserts that `policy` is refused before any folder check, as a matter that overlaps
/// `data_dir`, and that the ledger is as it was.
fn assert_overlaps(w: &World, policy: &Policy, data_dir: &Path) {
    let (mut ledger, before) = ledger_with_an_entry(w);
    let mut checked = 0;
    let check = |_: &Path| {
        checked += 1;
        Ok(FolderSupport::Ok)
    };
    let groups = FakeFolders::default();
    let refused = prepare(
        policy,
        data_dir,
        &[w.canonical("other/m2")],
        check,
        &groups,
        &mut ledger,
    );
    let Err(error @ Error::FolderUnsupported { path_kind, support }) = refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(path_kind, PathKind::Matter);
    assert_eq!(support, FolderRefusal::OverlapsDataFolder);
    assert_eq!(checked, 0, "refused before any folder check");
    assert!(groups.asked.borrow().is_empty());
    assert_eq!(std::fs::read(ledger.path()).unwrap(), before);
    let root = dunce::canonicalize(w.dir.path()).unwrap();
    let text = format!("{error} {error:?}");
    assert!(!text.contains(&*root.to_string_lossy()), "a path in {text}");
}

#[test]
fn a_matter_that_is_the_data_folder_is_refused() {
    let w = World::new("admit-data-is-matter");
    let policy = Policy::builder(w.at("data"), w.at("elsewhere"))
        .home(w.at("home"))
        .build()
        .unwrap();
    assert_overlaps(&w, &policy, &w.at("data"));
}

#[test]
fn a_matter_that_holds_the_data_folder_is_refused() {
    // Its Modify pair would reach <data_dir>\sandbox: the SID map, the ledger, the marker.
    let w = World::new("admit-matter-holds-data");
    w.dir.dir("matter/Clauseo");
    let policy = w.builder().build().unwrap();
    assert_overlaps(&w, &policy, &w.at("matter/Clauseo"));
}

#[test]
fn a_matter_inside_the_data_folder_is_refused() {
    let w = World::new("admit-matter-in-data");
    let policy = Policy::builder(w.at(STORE), w.at("elsewhere"))
        .home(w.at("home"))
        .build()
        .unwrap();
    assert_overlaps(&w, &policy, &w.at("data"));
}

#[test]
fn a_data_folder_reached_through_another_spelling_still_matches() {
    // `data_dir` is canonicalised: a `..` spelling (and on Windows a short or differently
    // cased one) is the same folder.
    let w = World::new("admit-data-spelling");
    let policy = Policy::builder(w.at(STORE), w.at("elsewhere"))
        .home(w.at("home"))
        .build()
        .unwrap();
    assert_overlaps(&w, &policy, &w.at("elsewhere/../data"));
}

#[test]
fn a_data_folder_that_doesnt_resolve_refuses() {
    let w = World::new("admit-data-missing");
    let policy = w.builder().build().unwrap();
    assert_overlaps(&w, &policy, &w.at("no-such-data"));
}

#[test]
fn a_matter_beside_the_data_folder_is_admitted() {
    // The tests' world: `<root>/matter` and `<root>/data` share a parent and nothing else.
    let w = World::new("admit-data-beside");
    let policy = w.builder().build().unwrap();
    let admitted = admit_with(
        &policy,
        &w.at("data"),
        &[],
        |_| Ok(FolderSupport::Ok),
        &FakeFolders::default(),
    );
    assert!(admitted.is_ok());
    // A data folder whose name only starts like the matter's isn't inside it.
    w.dir.dir("matter-data");
    let admitted = admit_with(
        &policy,
        &w.at("matter-data"),
        &[],
        |_| Ok(FolderSupport::Ok),
        &FakeFolders::default(),
    );
    assert!(admitted.is_ok());
}

/// Links where the gate reads names unresolved: the agents' folder, the credential link, the
/// Claude store's files.
mod links;

/// The gate with the real folder check, on CI's Windows job.
#[cfg(windows)]
mod on_windows;
