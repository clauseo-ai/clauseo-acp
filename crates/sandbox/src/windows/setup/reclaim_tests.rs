//! S15 `PR4i`'s clean-up on a real NTFS volume with a real second account. Two files that account
//! owns are taken by `--reclaim`'s walk (owner only: their DACLs unchanged, nothing granted):
//! one whose DACL lets the lawyer read and write but not change its owner (`PoolOwned`), and one
//! that lets only that account in (`ChildShut`). Then the lawyer's unelevated hand-back after
//! (`strip_taken`) takes that account's allow off both, and nothing else: the first is readable
//! and keeps its protection and the lawyer's entry; the second, left bare, is opened with the
//! owner's implied rights alone and inherits its folder's entries, so it is readable too. Only
//! in CI's elevated step (`#[ignore]`, and skipped at run time unless the token is elevated and
//! `CLAUSEO_ELEVATED_TESTS=1`). A second test plants an `OWNER RIGHTS` lock-out and follows it
//! through the walk, the ledger and the hand-back. The account carries this test's own prefix
//! ([`FAMILY`]) and a random suffix; the test first clears what a killed earlier try left, by
//! that prefix, and again when it ends.

use super::account_list::names_starting;
use super::accounts::{self, Kind};
use super::lock_tests::suffix;
use super::owner::Owner;
use super::password;
use super::taken_file;
use crate::test_support::TempDir;
use crate::windows::acl::privilege::{Privileges, RECLAIM};
use crate::windows::acl::take::NtfsOwners;
use crate::windows::acl::{NtfsAcl, owner};
use crate::windows::acl_plan::{AdmittedPlan, Grant, Inherit, Rights};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{Ledger, Stuck, ledger_path};
use crate::windows::matters::{MatterRow, Rows};
use crate::windows::reclaim::{Scope, TakenItem, hand_back_after, take_ownership};
use crate::windows::record::current_user_sid;
use crate::windows::sids::{Reserved, Sid};
use crate::windows::test_acl::{elevated_tests_enabled, has, owner_view, protected};
use crate::windows::test_dacl::{drop_privileges, give_owner, protect_with};
use crate::windows::test_world::matter_one;
use crate::windows::writer::{AclWriter, DenyStrip};

/// This test's account prefix (`ClauseoRcl`, then six hex digits).
const FAMILY: &str = "ClauseoRcl";

/// Deletes every local account whose name starts with [`FAMILY`].
fn sweep() {
    for name in names_starting(FAMILY, Kind::User).unwrap_or_default() {
        drop(accounts::delete_user(&name));
    }
}

/// Sweeps the family when dropped, so a failed assertion still cleans up.
struct Sweep;

impl Drop for Sweep {
    fn drop(&mut self) {
        sweep();
    }
}

#[test]
#[ignore = "elevated: makes a local account; CI's elevated step runs it"]
fn elevated_the_walk_takes_a_pool_users_shut_file_and_the_hand_back_cleans_it() {
    if !elevated_tests_enabled() {
        return;
    }
    sweep();
    let _sweep = Sweep;
    let name = format!("{FAMILY}{}", suffix());
    let other: Sid = accounts::create_user(&name, &password::generate().unwrap()).unwrap();
    let me = current_user_sid().unwrap();
    let dir = TempDir::new("reclaim-elevated");
    let kept = dunce::canonicalize(dir.file("matter/sub/kept.txt", "the agent's")).unwrap();
    let shut = dunce::canonicalize(dir.file("matter/sub/shut.txt", "the agent's")).unwrap();
    let matter = dunce::canonicalize(dir.path().join("matter")).unwrap();
    give_owner(&kept, &other).unwrap();
    give_owner(&shut, &other).unwrap();
    protect_with(&kept, &format!("D:P(A;;FA;;;{other})(A;;FRFW;;;{me})")).unwrap();
    protect_with(&shut, &format!("D:P(A;;FA;;;{other})")).unwrap();
    assert_eq!(owner(&shut).unwrap(), other);
    // The owner's token, taken before the privileges are enabled, as the lawyer's has none.
    let lawyer = Owner::of_this_process().unwrap();

    let walked = {
        let _privileges = Privileges::enable(&RECLAIM).unwrap();
        let granted = |descriptor: &[u8]| {
            lawyer
                .granted(descriptor)
                .map_err(|e| std::io::Error::other(e.to_string()))
        };
        let mut tree = NtfsOwners::new(&granted);
        let pool = [other.clone()];
        let scope = Scope {
            pool: &pool,
            owner: &me,
            bounds: std::slice::from_ref(&matter),
            profiles: &[],
        };
        take_ownership(&mut tree, &matter, true, &scope)
    };

    assert_eq!((walked.taken, walked.failed), (2, 0), "{walked:?}");
    for file in [&kept, &shut] {
        assert_eq!(owner(file).unwrap(), me);
        assert!(has(file, other.as_str(), false), "its DACL is as it was");
        assert!(protected(file));
    }
    assert!(!has(&shut, me.as_str(), false), "nothing granted");
    assert_eq!(
        owner(&matter).unwrap(),
        me,
        "the lawyer's own folder is untouched"
    );

    // The lawyer's unelevated hand-back, as a token without the privileges opens it.
    drop_privileges();
    let others = [other.clone()];
    let handed = NtfsAcl::new()
        .unwrap()
        .strip_taken(&matter, &others, &[], true, &DenyStrip::none())
        .unwrap();

    assert_eq!(handed.repaired, 2, "{handed:?}");
    for file in [&kept, &shut] {
        assert_eq!(owner(file).unwrap(), me);
        assert!(!has(file, other.as_str(), false) && !has(file, other.as_str(), true));
        assert_eq!(std::fs::read(file).unwrap(), b"the agent's");
    }
    assert!(
        protected(&kept),
        "a protection with the lawyer's entry in it stays"
    );
    assert!(has(&kept, me.as_str(), false), "the lawyer's entry stays");
    assert!(!protected(&shut), "left bare, it inherits");
}

/// An agent's `(D;;FA;;;WD)(D;;WO;;;OW)` shuts its file even to an owner, as `OWNER RIGHTS` takes
/// the owner's implied rights away. `--reclaim`'s walk takes the `OWNER RIGHTS` entry off with
/// the owner; the lawyer's hand-back after (`hand_back_after`, over a real ledger) strips the
/// `Everyone` deny on the item the walk reported, by its path and file ID, and clears its record;
/// the lawyer reads the file. The matter's own pool user is another SID, as when an agent's file
/// moved in from another matter.
#[test]
#[ignore = "elevated: makes a local account; CI's elevated step runs it"]
fn elevated_an_owner_rights_lock_out_is_lifted_by_reclaim_and_the_hand_back() {
    if !elevated_tests_enabled() {
        return;
    }
    sweep();
    let _sweep = Sweep;
    let name = format!("{FAMILY}{}", suffix());
    let other: Sid = accounts::create_user(&name, &password::generate().unwrap()).unwrap();
    let own = Sid::parse("S-1-5-21-9-9-9-1500").unwrap();
    let me = current_user_sid().unwrap();
    let dir = TempDir::new("reclaim-elevated-ow");
    let locked = dunce::canonicalize(dir.file("matter/sub/locked.txt", "the agent's")).unwrap();
    let matter = dunce::canonicalize(dir.path().join("matter")).unwrap();
    give_owner(&locked, &other).unwrap();
    protect_with(
        &locked,
        &format!("D:P(D;;FA;;;WD)(D;;WO;;;OW)(A;;FA;;;{other})"),
    )
    .unwrap();
    let lawyer = Owner::of_this_process().unwrap();
    let pool = [other.clone()];

    let (walked, taken) = {
        let _privileges = Privileges::enable(&RECLAIM).unwrap();
        let granted = |descriptor: &[u8]| {
            lawyer
                .granted(descriptor)
                .map_err(|e| std::io::Error::other(e.to_string()))
        };
        let mut tree = NtfsOwners::new(&granted);
        let scope = Scope {
            pool: &pool,
            owner: &me,
            bounds: std::slice::from_ref(&matter),
            profiles: &[],
        };
        let walked = take_ownership(&mut tree, &matter, true, &scope);
        (walked, tree.drain_taken())
    };

    assert_eq!((walked.taken, walked.failed), (1, 0), "{walked:?}");
    // Read as its owner alone: the Everyone deny still shuts every other open.
    let (still_protected, entries) = owner_view(&locked);
    let names = |sid: &str| entries.iter().any(|e| e.3.as_str() == sid);
    assert!(!names("S-1-3-4"), "OWNER RIGHTS' entry went with the owner");
    assert!(
        names("S-1-1-0") && names(other.as_str()),
        "nothing else changed"
    );
    assert!(still_protected);

    drop_privileges();
    let data = dir.dir("data");
    let trusted = [other.clone(), own.clone()];
    let reserved = Reserved::of_this_machine().unwrap();
    let mut ledger = Ledger::open(ledger_path(&data), &trusted, &reserved).unwrap();
    let root = Grant {
        path: matter,
        sid: own.clone(),
        rights: Rights::Modify,
        inherit: Inherit::Tree,
        kind: PathKind::Matter,
    };
    let mut plan = AdmittedPlan::from_grants(matter_one(), vec![root]);
    ledger.record_pending(&mut plan).unwrap();
    let held = MatterRow {
        pool_user: Some(own),
        ..MatterRow::default()
    };
    let rows = Rows::from([(matter_one(), held)]);
    let items: Vec<TakenItem> = taken
        .into_iter()
        .map(|(path, file)| TakenItem {
            matter: matter_one(),
            path,
            file,
        })
        .collect();
    let mut acl = NtfsAcl::new().unwrap();

    let handed = hand_back_after(&mut acl, &mut ledger, &rows, &items, &pool, &trusted).unwrap();

    assert_eq!(handed.refused, [], "{handed:?}");
    assert_eq!(owner(&locked).unwrap(), me);
    assert_eq!(
        std::fs::read(&locked).unwrap(),
        b"the agent's",
        "the lawyer opens it"
    );
    assert!(!has(&locked, "S-1-1-0", false), "the Everyone deny is gone");
    assert!(
        !has(&locked, other.as_str(), false),
        "the agent's allow is gone"
    );
    let taken_left = ledger.reclaims_owed().into_iter();
    assert_eq!(
        taken_left.filter(|o| o.why == Stuck::Taken).count(),
        0,
        "record cleared"
    );
}

/// A list of items taken that another account owns, shutting the lawyer out, isn't read: the
/// read is refused, so the hand-back adds nothing from it.
#[test]
#[ignore = "elevated: makes a local account; CI's elevated step runs it"]
fn elevated_a_list_another_account_owns_is_refused() {
    if !elevated_tests_enabled() {
        return;
    }
    sweep();
    let _sweep = Sweep;
    let name = format!("{FAMILY}{}", suffix());
    let other: Sid = accounts::create_user(&name, &password::generate().unwrap()).unwrap();
    let dir = TempDir::new("reclaim-elevated-list");
    let item = TakenItem {
        matter: matter_one(),
        path: dir.path().join("matter/doc.txt"),
        file: crate::windows::writer::FileId {
            volume: 1,
            id: [1; 16],
        },
    };
    taken_file::write(dir.path(), std::slice::from_ref(&item)).unwrap();
    let list = taken_file::path(dir.path());
    give_owner(&list, &other).unwrap();
    protect_with(&list, &format!("D:P(A;;FA;;;{other})")).unwrap();
    drop_privileges();

    assert!(
        taken_file::read(dir.path()).is_err(),
        "another account's list is refused"
    );
}
