//! The network set-up against the real WFP and firewall (S15 PR7), in [`Namespace::test`], so
//! no real set-up is touched. Elevated only: ignored unless CI's elevated step runs them with
//! `CLAUSEO_ELEVATED_TESTS=1`, and skipped without an elevated token. Each removes what it wrote,
//! on a failed assertion too. Elevated, the read grants are moot (the VM harness checks them as
//! the owner).

use super::check::{NetworkGap, check_setup};
use super::engine::{Engine, Entry, dacl_of};
use super::spec::{Namespace, NetworkSetup, Object};
use super::{check_launch_in, install, remove};
use crate::Error;
use crate::windows::record::current_user_sid;
use crate::windows::sids::Sid;
use crate::windows::test_dacl::elevated;

/// Whether an elevated test may run here: an elevated token and `CLAUSEO_ELEVATED_TESTS=1`.
fn enabled(test: &str) -> bool {
    let asked = std::env::var("CLAUSEO_ELEVATED_TESTS").is_ok_and(|value| value == "1");
    if asked && elevated() {
        return true;
    }
    eprintln!("skipped {test}: needs an elevated token and CLAUSEO_ELEVATED_TESTS=1");
    false
}

/// A made-up account SID: WFP and the firewall keep a descriptor naming it as written.
fn fake(rid: u32) -> Sid {
    Sid::parse(&format!("S-1-5-21-1-2-3-{rid}")).unwrap()
}

/// Removes the test namespace's objects when dropped.
struct Cleanup(Sid);

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Err(error) = remove(&Namespace::test(), Some(&self.0)) {
            eprintln!("the test's WFP objects weren't all removed: {error}");
        }
    }
}

/// Everyone, Users, Authenticated Users, INTERACTIVE: who must get no write right on our
/// objects.
const ANYONE: [&str; 4] = ["S-1-1-0", "S-1-5-32-545", "S-1-5-11", "S-1-5-4"];

/// The write rights: `FWPM_ACTRL_ADD`, `ADD_LINK`, `WRITE`, then `DELETE`, `WRITE_DAC`,
/// `WRITE_OWNER`, `GENERIC_ALL`, `GENERIC_WRITE`.
const WRITES: u32 = 0x1 | 0x2 | 0x400 | 0x1_0000 | 0x4_0000 | 0x8_0000 | 0x1000_0000 | 0x4000_0000;

/// Checks `object`'s DACL as set-up created it with `O:BA` and no DACL of its own: one is there
/// (not NULL), and no allow gives [`ANYONE`] a write right.
fn assert_dacl_holds(engine: &Engine, ns: &Namespace, object: Object) {
    let entries: Vec<Entry> = dacl_of(engine, ns, object)
        .unwrap()
        .unwrap_or_else(|| panic!("{object:?} has no DACL or a NULL one"));
    eprintln!("{object:?}: {entries:?}");
    assert!(!entries.is_empty(), "{object:?}'s DACL is empty");
    for (kind, mask, sid) in &entries {
        let deny = matches!(kind, 1 | 6 | 10 | 12);
        if deny || mask & WRITES == 0 {
            continue;
        }
        let sid = sid
            .as_deref()
            .unwrap_or_else(|| panic!("{object:?}: an allow of type {kind}"));
        assert!(
            !ANYONE.contains(&sid),
            "{object:?} lets {sid} write ({mask:#x})"
        );
    }
}

/// What a launch's check says for `user` now.
fn checked(user: &Sid) -> Result<(), NetworkGap> {
    let mut engine = Engine::open(false, "Clauseo sandbox test check").unwrap();
    check_setup(&Namespace::test(), user, &mut engine)
}

#[test]
#[ignore = "elevated: CI's elevated step only (CLAUSEO_ELEVATED_TESTS=1)"]
fn elevated_install_extend_check_and_remove() {
    if !enabled("elevated_install_extend_check_and_remove") {
        return;
    }
    let ns = Namespace::test();
    let owner = current_user_sid().unwrap();
    let cleanup = Cleanup(owner.clone());
    let (group, a, b, c) = (fake(4000), fake(4001), fake(4002), fake(4003));
    assert_eq!(checked(&a), Err(NetworkGap::NotSetUp), "a clean slate");

    let setup = |pool: &[Sid]| NetworkSetup::new(&owner, &group, pool).unwrap();
    let report = install(&ns, &setup(&[a.clone(), b.clone()])).unwrap();
    // WFP's filters are all there whatever the firewall says (it is only reported). This also
    // shows WFP takes NOT_EQUAL over a descriptor and a port range.
    assert_eq!(report.filters, 8);
    eprintln!("firewall: {:?}", report.firewall);
    let engine = Engine::open(false, "Clauseo sandbox test DACLs").unwrap();
    for object in [
        Object::Provider,
        Object::Sublayer,
        Object::Filter(ns.filter_key(0)),
    ] {
        assert_dacl_holds(&engine, &ns, object);
    }
    drop(engine);
    assert_eq!(checked(&a), Ok(()));
    assert_eq!(checked(&b), Ok(()));
    assert_eq!(checked(&c), Err(NetworkGap::PoolUserNotCovered));

    // A re-run changes nothing; an extend covers the new user.
    let again = install(&ns, &setup(&[b.clone(), a.clone()])).unwrap();
    assert_eq!(again.foreign_removed, 0);
    assert_eq!(checked(&a), Ok(()));
    install(&ns, &setup(&[a.clone(), b, c.clone()])).unwrap();
    assert_eq!(checked(&c), Ok(()));
    check_launch_in(&ns, &a, &[61090, 61091]).unwrap();
    match check_launch_in(&ns, &fake(4009), &[61090]) {
        Err(Error::NetworkUnfiltered {
            cause: NetworkGap::PoolUserNotCovered,
        }) => {}
        other => panic!("an uncovered user got {other:?}"),
    }

    // Removal, twice: the second finds nothing and is fine.
    let removed = remove(&ns, Some(&owner)).unwrap();
    assert_eq!(removed.firewall, Ok(()));
    assert_eq!(checked(&a), Err(NetworkGap::NotSetUp));
    remove(&ns, Some(&owner)).unwrap();
    drop(cleanup);
}
