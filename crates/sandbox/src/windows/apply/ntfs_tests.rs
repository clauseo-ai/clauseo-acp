//! [`apply`] on a real NTFS volume, with the ledger: rollbacks leave no entry, forgetting takes
//! everything back, `OWNER RIGHTS` holds an owner to it, and win-01's timing row. Temp folders and
//! synthetic SIDs; the owner tests use this process's own token as the file's owner.

use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt as _;
use std::path::Path;

use super::*;
use crate::windows::NtfsAcl;
use crate::windows::acl_plan::{Grant, LOW_LABEL_SID, OWNER_RIGHTS_SID};
use crate::windows::ledger::ledger_path;
use crate::windows::record::current_user_sid;
use crate::windows::test_acl::{Place, grant, has};
use crate::windows::test_dacl::protect_with;
use crate::windows::test_world::{OTHER_POOL_SIDS, USER_SID, matter_one, reserved, trusted};

fn ledger(place: &Place) -> Ledger {
    Ledger::open(ledger_path(&place.at("data")), &trusted(), &reserved()).unwrap()
}

/// The pool user's `Modify`, another pool user's deny, the label, and the owner rule.
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
        grant(matter, OWNER_RIGHTS_SID, Rights::Modify, Inherit::Tree),
        grant(
            matter,
            USER_SID,
            Rights::DenyChangePermissions,
            Inherit::Tree,
        ),
    ]
}

fn launch(place: &Place, matter: &Path) -> Result<Applied, Error> {
    let mut plan = AdmittedPlan::from_grants(matter_one(), launch_grants(matter));
    apply(&mut ledger(place), &mut plan, &mut NtfsAcl::new().unwrap())
}

/// Whether this process may open `path` to change its DACL.
#[expect(
    clippy::disallowed_methods,
    reason = "opens an existing test file for WRITE_DAC only"
)]
fn can_change_dacl(path: &Path) -> bool {
    const WRITE_DAC: u32 = 0x0004_0000;
    OpenOptions::new()
        .access_mode(WRITE_DAC)
        .custom_flags(0x0200_0000) // FILE_FLAG_BACKUP_SEMANTICS, for a folder too
        .open(path)
        .is_ok()
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
    for sid in [
        USER_SID,
        OTHER_POOL_SIDS[0],
        LOW_LABEL_SID,
        OWNER_RIGHTS_SID,
    ] {
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
    assert_eq!((done.removed, done.kept), (5, 0));
    for path in [&matter, &doc] {
        for sid in [
            USER_SID,
            OTHER_POOL_SIDS[0],
            LOW_LABEL_SID,
            OWNER_RIGHTS_SID,
        ] {
            assert!(!has(path, sid, true) && !has(path, sid, false), "{sid}");
        }
    }
    assert_eq!(ledger.entries().count(), 0);
}

/// The lawyer keeps full control of what the lawyer owns: the plan's `OWNER RIGHTS` entry
/// replaces only the owner's implicit rights, and the lawyer's inherited full control from the
/// profile still applies.
#[test]
fn the_lawyer_keeps_full_control_under_the_owner_rule() {
    let place = Place::new("apply-ntfs-lawyer");
    let doc = place.dir.file("matter/doc.txt", "the lawyer's");
    launch(&place, &place.at("matter")).unwrap();
    let late = place.dir.file("matter/late.txt", "made after the launch");
    let me = current_user_sid().unwrap();
    for path in [&doc, &late, &place.at("matter")] {
        assert!(can_change_dacl(path), "{path:?}");
        assert!(
            crate::windows::test_acl::entries(path)
                .iter()
                .any(|(kind, _, mask, sid)| *kind == 0
                    && *sid == me
                    && mask & 0x001f_01ff == 0x001f_01ff),
            "the lawyer's full control on {path:?}"
        );
    }
}

/// The VM row for the owner rule, with this process standing in for the agent: under an
/// `OWNER RIGHTS` `Modify` entry, the owner of a file it made can't change its DACL; with
/// only the implicit owner rights it could.
#[test]
fn an_owner_cant_change_the_dacl_of_what_it_made_under_the_owner_rule() {
    let place = Place::new("apply-ntfs-owner");
    place.dir.dir("ruled");
    place.dir.dir("open");
    protect_with(
        &place.at("ruled"),
        "D:P(A;OICI;0x1301bf;;;OW)(A;OICI;FA;;;SY)",
    )
    .unwrap();
    protect_with(&place.at("open"), "D:P(A;OICI;FA;;;SY)").unwrap();
    let ruled = place.dir.file("ruled/made.txt", "x");
    assert!(
        !can_change_dacl(&ruled),
        "OWNER RIGHTS replaced the implicit WRITE_DAC"
    );
    // Control: no rule, so the implicit owner rights give WRITE_DAC on the folder.
    assert!(can_change_dacl(&place.at("open")));
}

/// A `CREATOR OWNER` allow inherited from above (as on `C:\`) gives the maker full control;
/// the plan's inherited deny of `WRITE_DAC | WRITE_OWNER` for the pool user comes first.
#[test]
fn an_inherited_creator_owner_allow_cant_beat_the_deny_of_re_permissioning() {
    let place = Place::new("apply-ntfs-creator");
    let me = current_user_sid().unwrap();
    place.dir.dir("plain");
    place.dir.dir("denied");
    let creator = "(A;OICI;0x1301bf;;;OW)(A;OICIIO;FA;;;CO)(A;OICI;FA;;;SY)";
    protect_with(&place.at("plain"), &format!("D:P{creator}")).unwrap();
    protect_with(
        &place.at("denied"),
        &format!("D:P(D;OICI;0xc0000;;;{me}){creator}"),
    )
    .unwrap();
    let plain = place.dir.file("plain/made.txt", "x");
    let denied = place.dir.file("denied/made.txt", "x");
    assert!(
        can_change_dacl(&plain),
        "CREATOR OWNER alone gives the maker WRITE_DAC"
    );
    assert!(!can_change_dacl(&denied));
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
    apply(&mut ledger(&place), &mut plan, &mut NtfsAcl::new().unwrap()).unwrap();
    let sibling = started.elapsed();
    println!("files {count}: first {first:?}, next launch {again:?}, one sibling deny {sibling:?}");
}
