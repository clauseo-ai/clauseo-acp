//! The gate's admin copy as `arch::admin_copy_from` reads it, for the release without the Safe
//! workspace (decision 0035, "Safe workspace off for launch"): no record and no set-up program is
//! no admin copy, so the move to arm64 goes ahead; with the Safe workspace on, a present program
//! is compared as before.

use std::cell::Cell;
use std::io;

use clauseo_host::InstallKind;

use super::super::arch::{admin_copy_from, move_to_arm64};
use crate::admin_refresh::RefreshDecision;

const A: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const B: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

/// The installed set-up program is gone, as the hash of a missing file fails.
fn absent() -> io::Result<Option<String>> {
    Err(io::Error::from(io::ErrorKind::NotFound))
}

#[test]
fn no_record_and_no_set_up_program_is_no_admin_copy_and_the_move_goes_ahead() {
    let read = Cell::new(false);
    let admin = admin_copy_from(Ok::<_, ()>(None), || {
        read.set(true);
        absent()
    });
    assert_eq!(admin, Some(RefreshDecision::NotSetUp));
    // The missing program is never even looked at without a record.
    assert!(!read.get());
    assert!(move_to_arm64(true, InstallKind::CurrentUser, admin));
}

#[test]
fn with_a_record_a_present_set_up_program_is_compared_as_before() {
    let up_to_date = admin_copy_from(Ok::<_, ()>(Some(A.to_owned())), || Ok(Some(A.to_owned())));
    assert_eq!(up_to_date, Some(RefreshDecision::UpToDate));
    assert!(move_to_arm64(true, InstallKind::CurrentUser, up_to_date));
    let lagging = admin_copy_from(Ok::<_, ()>(Some(A.to_owned())), || Ok(Some(B.to_owned())));
    assert_eq!(lagging, Some(RefreshDecision::Refresh));
    assert!(!move_to_arm64(true, InstallKind::CurrentUser, lagging));
}

#[test]
fn a_record_whose_program_is_missing_or_an_unreadable_record_stays_x64() {
    // The copy's refresh can't be confirmed, so the install stays x64, as it always has.
    let missing = admin_copy_from(Ok::<_, ()>(Some(A.to_owned())), absent);
    assert_eq!(missing, None);
    assert!(!move_to_arm64(true, InstallKind::CurrentUser, missing));
    let unreadable = admin_copy_from(Err(()), absent);
    assert_eq!(unreadable, None);
}
