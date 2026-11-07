//! S15 `PR4i`'s clean-up on a real NTFS volume with a real second account. Two files that account
//! owns are taken by `--reclaim`'s walk (owner only: their DACLs unchanged, nothing granted):
//! one whose DACL lets the lawyer read and write but not change its owner (`PoolOwned`), and one
//! that lets only that account in (`ChildShut`). Then the lawyer's unelevated hand-back after
//! (`strip_taken`) takes that account's allow off both, and nothing else: the first is readable
//! and keeps its protection and the lawyer's entry; the second, left bare, is opened with the
//! owner's implied rights alone and inherits its folder's entries, so it is readable too. Only
//! in CI's elevated step (`#[ignore]`, and skipped at run time unless the token is elevated and
//! `CLAUSEO_ELEVATED_TESTS=1`). The account carries this test's own prefix ([`FAMILY`]) and a
//! random suffix; the test first clears what a killed earlier try left, by that prefix, and
//! again when it ends.

use super::account_list::names_starting;
use super::accounts::{self, Kind};
use super::lock_tests::suffix;
use super::owner::Owner;
use super::password;
use crate::test_support::TempDir;
use crate::windows::acl::privilege::{Privileges, RECLAIM};
use crate::windows::acl::take::NtfsOwners;
use crate::windows::acl::{NtfsAcl, owner};
use crate::windows::reclaim::{Scope, take_ownership};
use crate::windows::record::current_user_sid;
use crate::windows::sids::Sid;
use crate::windows::test_acl::{elevated_tests_enabled, has, protected};
use crate::windows::test_dacl::{drop_privileges, give_owner, protect_with};
use crate::windows::writer::AclWriter;

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
        .strip_taken(&matter, &others, &[], true)
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
