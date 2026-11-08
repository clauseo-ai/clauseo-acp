//! The LSA user-right calls (`preflight.rs`) on a real Windows, elevated only: CI's elevated step
//! runs them (`#[ignore]`, and skipped at run time unless the token is elevated and
//! `CLAUSEO_ELEVATED_TESTS=1`, D-B16). They make a local group with this family's own prefix
//! ([`FAMILY`]) and a random suffix, never the real pool group, and first clear what a killed
//! earlier try of this family left (split §8), by prefix: each such group's right, then the
//! group.

use super::account_list::names_starting;
use super::accounts::{self, Kind};
use super::answers::DENY_REMOTE_INTERACTIVE_RIGHT;
use super::lock_tests::suffix;
use super::preflight as rights;
use crate::windows::test_acl::elevated_tests_enabled;

/// The start of every group name this test family makes, and only this family: `ClauseoRght`,
/// then `G`, then six hex digits: 18 characters.
const FAMILY: &str = "ClauseoRght";

/// Takes the right off, then deletes, every group this family left; failures are ignored.
fn sweep() {
    for name in names_starting(FAMILY, Kind::Group).unwrap_or_default() {
        if let Ok(Some(sid)) = accounts::sid_of(&name, Kind::Group) {
            drop(rights::remove_right(&sid, DENY_REMOTE_INTERACTIVE_RIGHT));
        }
        drop(accounts::delete_group(&name));
    }
}

/// Sweeps when dropped, so a failed assertion still cleans up.
struct Sweep;

impl Drop for Sweep {
    fn drop(&mut self) {
        sweep();
    }
}

#[test]
#[ignore = "elevated: CI's elevated step runs it (CLAUSEO_ELEVATED_TESTS=1)"]
fn elevated_a_groups_remote_desktop_deny_is_granted_then_taken_off() {
    if !elevated_tests_enabled() {
        return;
    }
    sweep();
    let _sweep = Sweep;
    let name = format!("{FAMILY}G{}", suffix());
    assert!(name.len() <= 20, "{name}");
    let group = accounts::create_group(&name, "Clauseo rights test").unwrap();
    let holds = || {
        rights::holders(DENY_REMOTE_INTERACTIVE_RIGHT)
            .unwrap()
            .contains(&group)
    };

    // Not held yet: taking it off is a no-op.
    assert!(!holds());
    rights::remove_right(&group, DENY_REMOTE_INTERACTIVE_RIGHT).unwrap();

    // Granted, twice over: a right held already is fine.
    rights::add_right(&group, DENY_REMOTE_INTERACTIVE_RIGHT).unwrap();
    rights::add_right(&group, DENY_REMOTE_INTERACTIVE_RIGHT).unwrap();
    assert!(holds());

    // Taken off, twice over; then the group goes.
    rights::remove_right(&group, DENY_REMOTE_INTERACTIVE_RIGHT).unwrap();
    assert!(!holds());
    rights::remove_right(&group, DENY_REMOTE_INTERACTIVE_RIGHT).unwrap();
    accounts::delete_group(&name).unwrap();
}
