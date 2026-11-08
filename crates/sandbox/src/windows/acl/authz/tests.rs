//! `would_deny` on a real machine, elevated: a temporary account, a file its DACL grants it
//! read on, and one it grants nothing on.

use super::*;
use crate::windows::test_acl::{Place, elevated_tests_enabled};
use crate::windows::test_dacl::protect_with;
use crate::windows::test_pool::TestUser;

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_would_deny_follows_the_dacl_for_a_pool_user() {
    if !elevated_tests_enabled() {
        return;
    }
    let user = TestUser::new();
    let place = Place::new("authz-would-deny");
    let allowed = place.dir.file("allowed.txt", "x");
    let refused = place.dir.file("refused.txt", "x");
    protect_with(
        &allowed,
        &format!("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;{})", user.sid),
    )
    .unwrap();
    protect_with(&refused, "D:P(A;;FA;;;SY)(A;;FA;;;BA)").unwrap();

    let checker = Checker::new().unwrap();
    assert_eq!(
        checker.would_deny(&user.sid, &allowed, Access::Read),
        Some(false)
    );
    assert_eq!(
        checker.would_deny(&user.sid, &allowed, Access::Write),
        Some(true)
    );
    assert_eq!(
        checker.would_deny(&user.sid, &refused, Access::Read),
        Some(true)
    );
    assert_eq!(would_deny(&user.sid, &refused, Access::Write), Some(true));
    // A path that isn't there can't tell.
    let gone = place.at("gone.txt");
    assert_eq!(checker.would_deny(&user.sid, &gone, Access::Read), None);
}

#[test]
fn a_path_not_on_a_local_drive_is_refused_before_anything_opens() {
    let pool = Sid::parse("S-1-5-21-1-2-3-1101").unwrap();
    for path in [
        r"\\server\share\x.txt",
        r"\\?\UNC\server\share\x.txt",
        r"\\?\C:\x.txt",
        r"\\.\pipe\x",
        r"\Device\HarddiskVolume3\x",
        r"C:relative.txt",
        r"C:\a\..\..\x",
    ] {
        assert_eq!(
            would_deny(&pool, Path::new(path), Access::Read),
            None,
            "{path}"
        );
    }
}
