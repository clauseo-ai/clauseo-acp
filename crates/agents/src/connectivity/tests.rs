use super::is_hint;
use super::level::{CONSTRAINED_INTERNET_ACCESS, INTERNET_ACCESS};

// The levels below internet access (netioapi.h's `NetworkConnectivityLevelHint*`).
const UNKNOWN: i32 = 0;
const NONE: i32 = 1;
const LOCAL_ACCESS: i32 = 2;

#[test]
fn a_change_to_internet_access_from_another_level_is_a_hint() {
    for from in [UNKNOWN, NONE, LOCAL_ACCESS, CONSTRAINED_INTERNET_ACCESS] {
        assert!(is_hint(Some(from), INTERNET_ACCESS), "from {from}");
    }
    for from in [UNKNOWN, NONE, LOCAL_ACCESS, INTERNET_ACCESS] {
        assert!(
            is_hint(Some(from), CONSTRAINED_INTERNET_ACCESS),
            "from {from}"
        );
    }
}

#[test]
fn the_same_level_again_is_no_hint() {
    // A notice that changes only the cost or roaming carries the same level.
    assert!(!is_hint(Some(INTERNET_ACCESS), INTERNET_ACCESS));
    assert!(!is_hint(
        Some(CONSTRAINED_INTERNET_ACCESS),
        CONSTRAINED_INTERNET_ACCESS
    ));
}

#[test]
fn unknown_none_and_local_access_are_no_hint() {
    for to in [UNKNOWN, NONE, LOCAL_ACCESS] {
        for from in [UNKNOWN, NONE, LOCAL_ACCESS, INTERNET_ACCESS] {
            assert!(!is_hint(Some(from), to), "{from} to {to}");
        }
    }
}

#[test]
fn the_first_notice_only_records_the_level() {
    assert!(!is_hint(None, INTERNET_ACCESS));
    assert!(!is_hint(None, CONSTRAINED_INTERNET_ACCESS));
}
