//! [`apply`] on a real NTFS volume, with the ledger: rollbacks leave no entry, forgetting takes
//! everything back, and win-01's timing row. Temp folders and synthetic SIDs.

use std::path::{Path, PathBuf};

use super::*;
use crate::windows::NtfsAcl;
use crate::windows::acl_plan::{Grant, LOW_LABEL_SID};
use crate::windows::admit::{FolderList, ReadDir};
use crate::windows::ledger::ledger_path;
use crate::windows::test_acl::{Place, grant, has};
use crate::windows::test_dacl::protect_with;
use crate::windows::test_world::{OTHER_POOL_SIDS, USER_SID, matter_one, reserved, trusted};

fn ledger(place: &Place) -> Ledger {
    Ledger::open(ledger_path(&place.at("data")), &trusted(), &reserved()).unwrap()
}

/// The pool user's `Modify`, another pool user's deny, and the label.
fn launch_grants(matter: &Path) -> Vec<Grant> {
    vec![
        grant(matter, USER_SID, Rights::Modify, Inherit::Tree),
        grant(
            matter,
            OTHER_POOL_SIDS[0],
            Rights::DenyReadWrite,
            Inherit::Tree,
        ),
        grant(matter, LOW_LABEL_SID, Rights::LowLabel, Inherit::Tree),
    ]
}

/// Around a temp matter: the real listing, every folder `Ok`, set-asides refused, no
/// credentials file.
struct Around;

impl FolderList for Around {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        ReadDir.subfolders(parent)
    }
}

impl Surroundings for Around {
    fn check(&self, _folder: &Path) -> io::Result<crate::windows::FolderSupport> {
        Ok(crate::windows::FolderSupport::Ok)
    }

    fn set_aside(&mut self, _path: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn credentials(&self) -> Option<PathBuf> {
        None
    }
}

fn launch(place: &Place, matter: &Path) -> Result<Applied, Error> {
    let mut plan = AdmittedPlan::from_grants(matter_one(), launch_grants(matter));
    apply(
        &mut ledger(place),
        &mut plan,
        &mut NtfsAcl::new().unwrap(),
        &mut Around,
    )
}

#[test]
fn a_protected_subfolder_fails_the_launch_and_leaves_no_entry() {
    let place = Place::new("apply-ntfs-protected");
    place.dir.file("matter/sub/doc.txt", "x");
    protect_with(&place.at("matter/sub"), "D:P(A;OICI;FA;;;OW)").unwrap();
    let error = launch(&place, &place.at("matter")).unwrap_err();
    assert!(matches!(
        error,
        Error::AclApply {
            failure: ApplyFailure::ProtectedChild,
            ..
        }
    ));
    for sid in [USER_SID, OTHER_POOL_SIDS[0], LOW_LABEL_SID] {
        assert!(!has(&place.at("matter"), sid, false), "{sid}");
    }
    assert_eq!(ledger(&place).entries().count(), 0);
}

#[test]
fn a_launch_then_forget_leaves_nothing() {
    let place = Place::new("apply-ntfs-forget");
    let doc = place.dir.file("matter/sub/doc.txt", "x");
    let matter = place.at("matter");
    launch(&place, &matter).unwrap();
    assert!(has(&doc, USER_SID, true) && has(&doc, LOW_LABEL_SID, true));
    let mut ledger = ledger(&place);
    let done = forget_matter(&mut ledger, matter_one(), &mut NtfsAcl::new().unwrap()).unwrap();
    assert_eq!((done.removed, done.kept), (3, 0));
    for path in [&matter, &doc] {
        for sid in [USER_SID, OTHER_POOL_SIDS[0], LOW_LABEL_SID] {
            assert!(!has(path, sid, true) && !has(path, sid, false), "{sid}");
        }
    }
    assert_eq!(ledger.entries().count(), 0);
}

/// win-01's timing row (ignored on CI): the first propagation over `CLAUSEO_WALK_FILES` files
/// (50,000 by default) in 100 folders, the next launch's re-propagation, and one sibling's
/// deny (fix 2) over a tree as big. Run with `--run-ignored only -E 'test(timing)'`.
#[test]
#[ignore = "win-01 timing row; makes 50,000 files"]
#[expect(
    clippy::disallowed_methods,
    reason = "times the walk; the core never reads a clock"
)]
fn timing_first_propagation_and_fix_costs() {
    let count: usize = std::env::var("CLAUSEO_WALK_FILES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(50_000);
    let place = Place::new("apply-ntfs-timing");
    for i in 0..count {
        place
            .dir
            .file(&format!("matter/f{}/doc{i}.txt", i % 100), "x");
        place
            .dir
            .file(&format!("sibling/f{}/doc{i}.txt", i % 100), "x");
    }
    let matter = place.at("matter");
    let started = std::time::Instant::now();
    launch(&place, &matter).unwrap();
    let first = started.elapsed();
    let started = std::time::Instant::now();
    launch(&place, &matter).unwrap();
    let again = started.elapsed();
    let deny = Grant {
        kind: PathKind::Sibling,
        ..grant(
            &place.at("sibling"),
            OTHER_POOL_SIDS[1],
            Rights::DenyRead,
            Inherit::Tree,
        )
    };
    let mut plan = AdmittedPlan::from_grants(matter_one(), vec![deny]);
    let started = std::time::Instant::now();
    apply(
        &mut ledger(&place),
        &mut plan,
        &mut NtfsAcl::new().unwrap(),
        &mut Around,
    )
    .unwrap();
    let sibling = started.elapsed();
    println!("files {count}: first {first:?}, next launch {again:?}, one sibling deny {sibling:?}");
}
