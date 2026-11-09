use super::{RdPolicy, decide};
use clauseo_protocol::ToolsUnavailableReason::{BrowserBlockedByPolicy, BrowserPolicyUnreadable};
use clauseo_settings::chromium_policy::{Browser, Found, Place, RemoteDebugging, Scope};

fn read(found: &[Found]) -> RemoteDebugging {
    let place = Place {
        browser: Browser::Chrome,
        scope: Scope::Machine,
    };
    RemoteDebugging {
        found: found.iter().map(|said| (place, said.clone())).collect(),
    }
}

fn unreadable() -> Found {
    Found::Unreadable("the registry read failed (5)".to_owned())
}

fn bad() -> Found {
    Found::BadValue("the REG_SZ is not an integer".to_owned())
}

#[test]
fn a_false_blocks_unless_the_firm_allows_clauseo_anyway() {
    let off = read(&[Found::Absent, Found::Disallowed]);
    assert_eq!(decide(&off, false), Some(BrowserBlockedByPolicy));
    assert_eq!(decide(&off, true), None);
}

#[test]
fn a_read_error_blocks_with_its_own_reason_unless_the_firm_allows_clauseo_anyway() {
    let failed = read(&[Found::Allowed, unreadable()]);
    assert_eq!(decide(&failed, false), Some(BrowserPolicyUnreadable));
    assert_eq!(decide(&failed, true), None);
}

#[test]
fn a_false_wins_over_a_read_error() {
    for found in [
        [unreadable(), Found::Disallowed],
        [Found::Disallowed, unreadable()],
    ] {
        assert_eq!(decide(&read(&found), false), Some(BrowserBlockedByPolicy));
        assert_eq!(decide(&read(&found), true), None);
    }
}

#[test]
fn nothing_set_true_or_a_bad_value_does_not_block() {
    for found in [
        &[][..],
        &[Found::Absent],
        &[Found::Allowed],
        &[bad()],
        &[bad(), Found::Allowed],
    ] {
        assert_eq!(decide(&read(found), false), None, "{found:?}");
    }
}

#[test]
fn the_wired_reader_in_tests_finds_nothing_set() {
    let wired = RdPolicy::wired();
    assert_eq!(format!("{:?}", wired.reader), "NothingSet");
    assert_eq!(wired.check(false), None);
    let wiring = crate::config::LaunchWiring::default();
    assert_eq!(format!("{:?}", wiring.rd_policy.reader), "NothingSet");
}
