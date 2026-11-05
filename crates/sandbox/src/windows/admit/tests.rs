use std::io;
use std::path::{Path, PathBuf};

use super::*;
use crate::windows::acl_plan::{AdmittedPlan, plan};
use crate::windows::ledger::{Ledger, ledger_path};
use crate::windows::test_world::{
    FakeGroups, NOTES, STATE, STORE, World, matter_one, matter_two, reserved, sids, user_sid,
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
    groups: &FakeGroups,
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
    let mut ledger = Ledger::open(&path, &user_sid(), &reserved()).unwrap();
    let other = w.builder().build().unwrap();
    let admitted = admit_with(
        &other,
        &w.at("data"),
        &[],
        |_| Ok(FolderSupport::Ok),
        &FakeGroups::default(),
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
    admit_with(&policy, &w.at("data"), &[], check, &FakeGroups::default()).unwrap();
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
            let error =
                admit_with(&policy, &w.at("data"), &[], check, &FakeGroups::default()).unwrap_err();
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
            &FakeGroups::default(),
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
        &FakeGroups::default(),
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
    let mut ledger = Ledger::open(&path, &user_sid(), &reserved()).unwrap();
    let check = answering(w.canonical("matter"), || Ok(FolderSupport::NoPermissions));
    assert!(
        prepare(
            &policy,
            &w.at("data"),
            &[],
            check,
            &FakeGroups::default(),
            &mut ledger
        )
        .is_err()
    );
    assert!(!path.exists());
    assert!(!w.at("data/sandbox").exists());
}

#[test]
fn other_matters_a_group_can_reach_are_closed() {
    let w = World::new("admit-others");
    let policy = w.builder().build().unwrap();
    let (m2, m3) = (w.canonical("other/m2"), w.canonical("other/m3"));
    let unread = w.at("other/unread");
    let gone = w.at("other/gone");
    let groups = FakeGroups {
        open: vec![m2.clone()],
        failing: vec![unread.clone()],
        missing: vec![gone.clone()],
        ..FakeGroups::default()
    };
    let others = [m2.clone(), m3, unread.clone(), gone];
    let admitted = admit_with(
        &policy,
        &w.at("data"),
        &others,
        |_| Ok(FolderSupport::Ok),
        &groups,
    )
    .unwrap();
    // A DACL that can't be read counts as open; a missing folder has nothing to close.
    assert_eq!(admitted.closed(), [m2, unread]);
    assert_eq!(groups.asked.borrow().len(), 4);
}

#[test]
fn another_matter_that_cant_be_closed_refuses_the_launch() {
    let w = World::new("admit-others-refused");
    let policy = w.builder().build().unwrap();
    let m2 = w.canonical("other/m2");
    let groups = FakeGroups {
        open: vec![m2.clone()],
        ..FakeGroups::default()
    };
    let (mut ledger, before) = ledger_with_an_entry(&w);
    for answer in [
        FolderSupport::CantChangePermissions,
        FolderSupport::NoPermissions,
    ] {
        let check = |path: &Path| {
            Ok(if path == m2 {
                answer
            } else {
                FolderSupport::Ok
            })
        };
        let error = prepare(
            &policy,
            &w.at("data"),
            std::slice::from_ref(&m2),
            check,
            &groups,
            &mut ledger,
        )
        .unwrap_err();
        let Error::FolderUnsupported { path_kind, support } = error else {
            panic!("{error:?}");
        };
        assert_eq!(path_kind, PathKind::OtherMatter);
        assert_eq!(support, FolderRefusal::Support(answer));
        assert_eq!(std::fs::read(ledger.path()).unwrap(), before);
    }
}

#[test]
fn another_matter_on_a_share_or_gone_is_left_alone() {
    let w = World::new("admit-others-network");
    let policy = w.builder().build().unwrap();
    let m2 = w.canonical("other/m2");
    let groups = FakeGroups {
        open: vec![m2.clone()],
        ..FakeGroups::default()
    };
    let network = |path: &Path| {
        Ok(if path == m2 {
            FolderSupport::Network
        } else {
            FolderSupport::Ok
        })
    };
    let admitted = admit_with(
        &policy,
        &w.at("data"),
        std::slice::from_ref(&m2),
        network,
        &groups,
    )
    .unwrap();
    assert_eq!(admitted.closed(), [] as [PathBuf; 0]);
    let gone = |path: &Path| {
        if path == m2 {
            Err(io::Error::from(io::ErrorKind::NotFound))
        } else {
            Ok(FolderSupport::Ok)
        }
    };
    let admitted = admit_with(
        &policy,
        &w.at("data"),
        std::slice::from_ref(&m2),
        gone,
        &groups,
    )
    .unwrap();
    assert_eq!(admitted.closed(), [] as [PathBuf; 0]);
}

#[test]
fn other_matters_are_not_read_when_a_grant_folder_is_refused() {
    let w = World::new("admit-order");
    let policy = w.builder().build().unwrap();
    let groups = FakeGroups::default();
    let check = answering(w.canonical("matter"), || Ok(FolderSupport::Network));
    let others = [w.canonical("other/m2")];
    assert!(admit_with(&policy, &w.at("data"), &others, check, &groups).is_err());
    assert!(groups.asked.borrow().is_empty());
}

#[test]
fn another_matter_nested_with_this_one_is_refused_whatever_its_dacl() {
    // Matter `m` and another matter `m\B` inside it, then the other way round. No group can
    // reach either (the DACL reader says no), and the folder checks all say Ok.
    let w = World::new("admit-nested");
    w.dir.dir("matter/B");
    let policy = w.builder().build().unwrap();
    let (mut ledger, before) = ledger_with_an_entry(&w);
    let inner = w.canonical("matter/B");
    let groups = FakeGroups::default();
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
        "refused before the DACL is read"
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
    let groups = FakeGroups::default();
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
        &FakeGroups::default(),
    );
    assert!(admitted.is_ok());
    // A data folder whose name only starts like the matter's isn't inside it.
    w.dir.dir("matter-data");
    let admitted = admit_with(
        &policy,
        &w.at("matter-data"),
        &[],
        |_| Ok(FolderSupport::Ok),
        &FakeGroups::default(),
    );
    assert!(admitted.is_ok());
}

/// Links where the gate reads names unresolved: the agents' folder, the credential link, the
/// Claude store's files.
mod links;

/// The gate with the real folder check, on CI's Windows job.
#[cfg(windows)]
mod on_windows;
