use super::decide;
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

#[test]
fn a_false_blocks_unless_the_firm_allows_clauseo_anyway() {
    let off = read(&[Found::Absent, Found::Disallowed]);
    assert!(decide(&off, false));
    assert!(!decide(&off, true));
}

#[test]
fn absent_allowed_or_unreadable_does_not_block() {
    let unreadable = Found::Unreadable("the registry read failed (5)".to_owned());
    for found in [&[][..], &[Found::Absent], &[Found::Allowed], &[unreadable]] {
        assert!(!decide(&read(found), false), "{found:?}");
    }
}
