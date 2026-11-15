use std::path::PathBuf;

use super::{RdPolicy, decide, decide_among};
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

/// A reader that says what it was given, every time.
#[derive(Debug)]
struct Says(Vec<Found>);

impl clauseo_settings::chromium_policy::RemoteDebuggingPolicy for Says {
    fn read(&self) -> RemoteDebugging {
        read(&self.0)
    }
}

/// The lawyer's "Try again" reads with the same rule as a launch, the opt-in included.
#[test]
fn a_recheck_reads_with_the_launch_rule() {
    let cases = [
        (vec![unreadable()], Some(BrowserPolicyUnreadable)),
        (
            vec![unreadable(), Found::Disallowed],
            Some(BrowserBlockedByPolicy),
        ),
        (vec![Found::Allowed, bad()], None),
    ];
    for (found, reason) in cases {
        let policy = RdPolicy::new(std::sync::Arc::new(Says(found.clone())));
        assert_eq!(policy.recheck(false), reason, "{found:?}");
        assert_eq!(policy.recheck(false), policy.check(false), "{found:?}");
        assert_eq!(policy.recheck(true), None, "{found:?}");
    }
}

/// Windows: what each place said, by browser and scope.
fn per(found: &[(Browser, Scope, Found)]) -> RemoteDebugging {
    RemoteDebugging {
        found: found
            .iter()
            .map(|(browser, scope, said)| {
                let place = Place {
                    browser: *browser,
                    scope: *scope,
                };
                (place, said.clone())
            })
            .collect(),
    }
}

fn edge() -> (Browser, PathBuf) {
    (Browser::Edge, PathBuf::from("msedge.exe"))
}

fn chrome() -> (Browser, PathBuf) {
    (Browser::Chrome, PathBuf::from("chrome.exe"))
}

#[test]
fn windows_edge_blocked_and_chrome_allowed_takes_chrome() {
    let read = per(&[(Browser::Edge, Scope::Machine, Found::Disallowed)]);
    let both = [edge(), chrome()];
    assert_eq!(decide_among(&read, &both, false), Ok(Some(chrome().1)));
    // The user's place blocks Edge as the computer's does.
    let read = per(&[(Browser::Edge, Scope::User, Found::Disallowed)]);
    assert_eq!(decide_among(&read, &both, false), Ok(Some(chrome().1)));
    // Nothing set takes Edge, first.
    assert_eq!(decide_among(&per(&[]), &both, false), Ok(Some(edge().1)));
}

#[test]
fn windows_both_blocked_is_blocked_unless_the_firm_allows_clauseo_anyway() {
    let read = per(&[
        (Browser::Edge, Scope::User, Found::Disallowed),
        (Browser::Chrome, Scope::Machine, Found::Disallowed),
    ]);
    let both = [edge(), chrome()];
    assert_eq!(
        decide_among(&read, &both, false),
        Err(BrowserBlockedByPolicy)
    );
    assert_eq!(decide_among(&read, &both, true), Ok(Some(edge().1)));
}

#[test]
fn windows_edge_only_installed_and_blocked_is_blocked() {
    let read = per(&[
        (Browser::Edge, Scope::Machine, Found::Disallowed),
        (Browser::Chrome, Scope::Machine, Found::Allowed),
    ]);
    assert_eq!(
        decide_among(&read, &[edge()], false),
        Err(BrowserBlockedByPolicy)
    );
}

#[test]
fn windows_nothing_installed_is_no_policy_block() {
    let read = per(&[
        (Browser::Edge, Scope::Machine, Found::Disallowed),
        (Browser::Chrome, Scope::User, unreadable()),
    ]);
    assert_eq!(decide_among(&read, &[], false), Ok(None));
    assert_eq!(decide_among(&read, &[], true), Ok(None));
}

#[test]
fn windows_an_unreadable_browser_is_passed_over_and_a_false_wins_over_it() {
    let both = [edge(), chrome()];
    let edge_unread = per(&[(Browser::Edge, Scope::User, unreadable())]);
    assert_eq!(
        decide_among(&edge_unread, &both, false),
        Ok(Some(chrome().1))
    );
    let all_unread = per(&[
        (Browser::Edge, Scope::User, unreadable()),
        (Browser::Chrome, Scope::Machine, unreadable()),
    ]);
    assert_eq!(
        decide_among(&all_unread, &both, false),
        Err(BrowserPolicyUnreadable)
    );
    assert_eq!(decide_among(&all_unread, &both, true), Ok(Some(edge().1)));
    let mixed = per(&[
        (Browser::Edge, Scope::User, unreadable()),
        (Browser::Chrome, Scope::Machine, Found::Disallowed),
    ]);
    assert_eq!(
        decide_among(&mixed, &both, false),
        Err(BrowserBlockedByPolicy)
    );
}

/// Neither browser Clauseo starts on Windows reads Chromium's place, so it decides nothing there.
#[test]
fn windows_ignores_chromiums_place() {
    let read = per(&[
        (Browser::Chromium, Scope::Machine, Found::Disallowed),
        (Browser::Chromium, Scope::User, unreadable()),
        (Browser::Edge, Scope::Machine, bad()),
    ]);
    assert_eq!(
        decide_among(&read, &[edge(), chrome()], false),
        Ok(Some(edge().1))
    );
}

#[test]
fn a_windows_recheck_reads_with_the_launch_rule() {
    let found = vec![Found::Disallowed];
    // `Says` puts everything at Chrome's place.
    let policy = RdPolicy::new(std::sync::Arc::new(Says(found)));
    assert_eq!(policy.recheck_among(false, &[edge(), chrome()]), None);
    assert_eq!(
        policy.recheck_among(false, &[chrome()]),
        Some(BrowserBlockedByPolicy)
    );
    assert_eq!(policy.recheck_among(true, &[chrome()]), None);
    assert_eq!(policy.recheck_among(false, &[]), None);
    assert_eq!(policy.pick(false, &[chrome()]), Err(BrowserBlockedByPolicy));
    assert_eq!(policy.pick(false, &[edge(), chrome()]), Ok(Some(edge().1)));
}
