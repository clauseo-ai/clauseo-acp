//! S15 `PR4h`'s hand-back on a real NTFS volume with a real second account: a child another
//! pool user owns, inside a matter's tree, goes to the desktop user, losing that user's explicit
//! allow and taking the inherited deny; one whose hand-back lacks `WRITE_OWNER` is refused as
//! `PoolOwned { owned: Child }`. Only in CI's elevated step (`#[ignore]`, and skipped at run time
//! unless the token is elevated and `CLAUSEO_ELEVATED_TESTS=1`). Each test's account carries
//! that test's own prefix ([`FAMILY_A`], [`FAMILY_B`]) and a random suffix, and each test first
//! clears what a killed earlier try of it left, by its prefix.

use std::path::{Path, PathBuf};

use super::account_list::names_starting;
use super::accounts::{self, Kind};
use super::lock_tests::suffix;
use super::password;
use crate::test_support::TempDir;
use crate::windows::acl::{NtfsAcl, owner};
use crate::windows::acl_plan::{Grant, Inherit, Rights};
use crate::windows::admit::PathKind;
use crate::windows::record::current_user_sid;
use crate::windows::sids::Sid;
use crate::windows::test_acl::{elevated_tests_enabled, has};
use crate::windows::test_dacl::{auto_inherit_with, drop_privileges, give_owner, protect_with};
use crate::windows::test_world::OTHER_POOL_SIDS;
use crate::windows::writer::{AclWriter, ApplyFailure, Owned, Reach, Strict};

/// Each test's own account prefix (`ClauseoHbkA` or `ClauseoHbkB`, then six hex digits), so
/// one test's sweep never deletes the other's account.
const FAMILY_A: &str = "ClauseoHbkA";
const FAMILY_B: &str = "ClauseoHbkB";

/// Deletes every local account whose name starts with `family`.
fn sweep(family: &str) {
    for name in names_starting(family, Kind::User).unwrap_or_default() {
        drop(accounts::delete_user(&name));
    }
}

/// Sweeps its family when dropped, so a failed assertion still cleans up.
struct Sweep(&'static str);

impl Drop for Sweep {
    fn drop(&mut self) {
        sweep(self.0);
    }
}

/// A fresh local account of `family`: the other matter's pool user. Sweeps that family first
/// (what a killed earlier try left) and again when the guard drops.
fn other_account(family: &'static str) -> (Sweep, Sid) {
    sweep(family);
    let guard = Sweep(family);
    let password = password::generate().unwrap();
    let name = format!("{family}{}", suffix());
    (guard, accounts::create_user(&name, &password).unwrap())
}

/// A matter folder in a temp folder whose `item` (a file, made) another account `other` owns
/// and then gets `dacl` as its DACL; the matter denies `other` reads and writes for the whole
/// tree. The process then drops its backup and restore privileges, as the lawyer's token has
/// none.
fn matter_with(test: &str, item: &str, other: &Sid, dacl: &dyn Fn(&Path)) -> (TempDir, PathBuf) {
    let dir = TempDir::new(test);
    let made = dunce::canonicalize(dir.file(&format!("matter/{item}"), "the agent's")).unwrap();
    let matter = dunce::canonicalize(dir.path().join("matter")).unwrap();
    let mut acl = NtfsAcl::new().unwrap();
    let deny = Grant {
        path: matter,
        sid: other.clone(),
        rights: Rights::DenyReadWrite,
        inherit: Inherit::Tree,
        kind: PathKind::OtherMatter,
    };
    acl.add(&deny).unwrap();
    give_owner(&made, other).unwrap();
    dacl(&made);
    drop_privileges();
    (dir, made)
}

/// The matter's strict walk with `acl`, its own pool user a made-up SID, `other` another pool
/// user.
fn walk(
    acl: &mut NtfsAcl,
    dir: &TempDir,
    other: &Sid,
) -> Result<crate::windows::writer::Walked, ApplyFailure> {
    let own = Sid::parse(OTHER_POOL_SIDS[0]).unwrap();
    let strict = Strict {
        watched: vec![other.clone()],
        pool: vec![own.clone(), other.clone()],
        assigned: vec![own],
    };
    let matter = dunce::canonicalize(dir.path().join("matter")).unwrap();
    acl.propagate(&matter, Reach::Tree, Some(&strict))
}

#[test]
#[ignore = "elevated: makes a local account; CI's elevated step runs it"]
fn elevated_another_pool_users_child_goes_to_the_desktop_user_without_its_allow() {
    if !elevated_tests_enabled() {
        return;
    }
    let (_sweep, other) = other_account(FAMILY_A);
    let allow = format!("D:(A;;FA;;;{other})");
    let set = |path: &Path| auto_inherit_with(path, &allow).unwrap();
    let (dir, doc) = matter_with("handback-elevated", "moved/doc.txt", &other, &set);
    assert_eq!(owner(&doc).unwrap(), other);
    assert!(has(&doc, other.as_str(), false), "its explicit allow");

    let walked = walk(&mut NtfsAcl::new().unwrap(), &dir, &other).unwrap();

    assert!(walked.repaired >= 1, "{walked:?}");
    assert_eq!(owner(&doc).unwrap(), current_user_sid().unwrap());
    assert!(
        !has(&doc, other.as_str(), false),
        "its explicit allow is gone"
    );
    assert!(
        has(&doc, other.as_str(), true),
        "the matter's deny reaches it"
    );
}

/// The lawyer may read the child and change its DACL, not its owner: the hand-back is refused
/// for want of `WRITE_OWNER`, as `PoolOwned { owned: Child }`, naming it, and nothing changes.
#[test]
#[ignore = "elevated: makes a local account; CI's elevated step runs it"]
fn elevated_a_child_whose_owner_cant_be_changed_is_refused_as_pool_owned() {
    if !elevated_tests_enabled() {
        return;
    }
    let (_sweep, other) = other_account(FAMILY_B);
    let me = current_user_sid().unwrap();
    let only_read_and_dac = format!("D:P(A;;FRWD;;;{me})");
    let set = |path: &Path| protect_with(path, &only_read_and_dac).unwrap();
    let (dir, doc) = matter_with("handback-elevated-shut", "kept.txt", &other, &set);
    let mut acl = NtfsAcl::new().unwrap();

    let walked = walk(&mut acl, &dir, &other);

    let child = ApplyFailure::PoolOwned {
        owned: Owned::Child,
    };
    assert_eq!(walked, Err(child));
    let at = acl.refused_at().unwrap();
    assert!(at.ends_with("kept.txt"), "{at:?}");
    assert_eq!(owner(&doc).unwrap(), other);
}
