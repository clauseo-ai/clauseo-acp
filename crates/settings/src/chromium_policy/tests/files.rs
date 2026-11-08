//! The macOS managed preferences reader and the Linux folders reader over a fake tree: each
//! file says nothing, false, true, a bad value or a read error, and the Linux caps are read
//! errors.

use super::{Browser, FakeTree, Found, Kind, Scope, at, kind, others_absent, place};
use crate::chromium_policy::{
    LINUX_DIRS, MAX_FILE_BYTES, MAX_FILES, read_linux, read_managed_prefs,
};
use std::io;
use std::path::PathBuf;

/// A managed preferences file with a homepage and, if given, `RemoteDebuggingAllowed`.
pub(super) fn plist(value: Option<plist::Value>) -> Vec<u8> {
    let mut dict = plist::Dictionary::new();
    dict.insert("HomepageLocation".to_owned(), "https://firm.example".into());
    if let Some(value) = value {
        dict.insert(crate::chromium_policy::VALUE.to_owned(), value);
    }
    let mut bytes = Vec::new();
    plist::Value::Dictionary(dict)
        .to_writer_xml(&mut bytes)
        .unwrap();
    bytes
}

const MAC_FILES: [(&str, Browser, Scope); 6] = [
    (
        "/Library/Managed Preferences/com.google.Chrome.plist",
        Browser::Chrome,
        Scope::Machine,
    ),
    (
        "/Library/Managed Preferences/com.microsoft.Edge.plist",
        Browser::Edge,
        Scope::Machine,
    ),
    (
        "/Library/Managed Preferences/org.chromium.Chromium.plist",
        Browser::Chromium,
        Scope::Machine,
    ),
    (
        "/Library/Managed Preferences/lawyer/com.google.Chrome.plist",
        Browser::Chrome,
        Scope::User,
    ),
    (
        "/Library/Managed Preferences/lawyer/com.microsoft.Edge.plist",
        Browser::Edge,
        Scope::User,
    ),
    (
        "/Library/Managed Preferences/lawyer/org.chromium.Chromium.plist",
        Browser::Chromium,
        Scope::User,
    ),
];

#[test]
fn each_managed_preferences_file_says_nothing_false_true_bad_or_unreadable() {
    let cases: [(Result<Vec<u8>, io::ErrorKind>, Kind); 9] = [
        (Ok(plist(None)), Kind::Absent),
        (Ok(plist(Some(false.into()))), Kind::Disallowed),
        (Ok(plist(Some(true.into()))), Kind::Allowed),
        (Ok(plist(Some(0.into()))), Kind::Disallowed),
        (Ok(plist(Some(1.into()))), Kind::Allowed),
        (Ok(plist(Some(2.into()))), Kind::Bad),
        (Ok(plist(Some("false".into()))), Kind::Bad),
        (Ok(b"not a plist".to_vec()), Kind::Bad),
        (Err(io::ErrorKind::PermissionDenied), Kind::Unreadable),
    ];
    for (path, browser, scope) in MAC_FILES {
        for (bytes, expected) in &cases {
            let tree = FakeTree::default().with(path, bytes.as_deref().map_err(|kind| *kind));
            let read = read_managed_prefs(&tree, Ok(Some("lawyer".to_owned())));
            assert_eq!(read.found.len(), 6);
            assert_eq!(kind(&at(&read, browser, scope)), *expected, "{path}");
            assert!(others_absent(&read, browser, scope), "{path}");
            let off = (*expected == Kind::Disallowed).then_some(place(browser, scope));
            assert_eq!(read.turned_off_by(), off, "{path}");
        }
    }
}

#[test]
fn without_a_login_user_the_users_files_are_read_errors() {
    let off = plist(Some(false.into()));
    let tree = FakeTree::default().with(MAC_FILES[1].0, Ok(&off));
    for looked_up in [
        Ok(None),
        Err(io::Error::from(io::ErrorKind::Other)),
        Ok(Some("..".to_owned())),
    ] {
        let read = read_managed_prefs(&tree, looked_up);
        assert_eq!(
            read.turned_off_by(),
            Some(place(Browser::Edge, Scope::Machine))
        );
        for browser in [Browser::Chrome, Browser::Edge, Browser::Chromium] {
            assert_eq!(
                kind(&at(&read, browser, Scope::User)),
                Kind::Unreadable,
                "{browser:?}"
            );
        }
        assert_eq!(at(&read, Browser::Chrome, Scope::Machine), Found::Absent);
    }
}

#[test]
fn each_linux_managed_folder_says_nothing_false_true_bad_or_unreadable() {
    // Edge's Linux policy folder is `/etc/opt/edge`, not its install folder's `microsoft/msedge`.
    assert_eq!(
        LINUX_DIRS,
        [
            (Browser::Chrome, "/etc/opt/chrome/policies/managed"),
            (Browser::Edge, "/etc/opt/edge/policies/managed"),
            (Browser::Chromium, "/etc/chromium/policies/managed"),
        ]
    );
    let cases: [(Result<&[u8], io::ErrorKind>, Kind); 9] = [
        (Ok(b"{}"), Kind::Absent),
        (
            Ok(br#"{"RemoteDebuggingAllowed": false}"#),
            Kind::Disallowed,
        ),
        (Ok(br#"{"RemoteDebuggingAllowed": true}"#), Kind::Allowed),
        (Ok(br#"{"RemoteDebuggingAllowed": 0}"#), Kind::Disallowed),
        (Ok(br#"{"RemoteDebuggingAllowed": 1}"#), Kind::Allowed),
        (Ok(br#"{"RemoteDebuggingAllowed": "false"}"#), Kind::Bad),
        (Ok(b"{ not json"), Kind::Bad),
        (Err(io::ErrorKind::PermissionDenied), Kind::Unreadable),
        (Err(io::ErrorKind::Other), Kind::Unreadable),
    ];
    for (browser, dir) in LINUX_DIRS {
        for (bytes, expected) in &cases {
            let tree = FakeTree::default().with(&format!("{dir}/firm.json"), *bytes);
            let read = read_linux(&tree);
            assert_eq!(read.found.len(), 3);
            assert_eq!(
                kind(&at(&read, browser, Scope::Machine)),
                *expected,
                "{dir}"
            );
            assert!(others_absent(&read, browser, Scope::Machine), "{dir}");
            let off = (*expected == Kind::Disallowed).then_some(place(browser, Scope::Machine));
            assert_eq!(read.turned_off_by(), off, "{dir}");
        }
        // Nothing there, a folder that cannot be listed, and the recommended folder.
        assert_eq!(
            at(&read_linux(&FakeTree::default()), browser, Scope::Machine),
            Found::Absent
        );
        let mut broken = FakeTree::default();
        broken.broken_dirs.push(PathBuf::from(dir));
        let said = at(&read_linux(&broken), browser, Scope::Machine);
        assert_eq!(kind(&said), Kind::Unreadable, "{dir}");
        // Chrome does not take this policy as a recommendation, so neither does Clauseo.
        let recommended = dir.replace("/managed", "/recommended/firm.json");
        let tree =
            FakeTree::default().with(&recommended, Ok(br#"{"RemoteDebuggingAllowed": false}"#));
        assert_eq!(read_linux(&tree).turned_off_by(), None);
    }
}

#[test]
fn a_linux_folder_says_once_per_file_and_skips_other_files() {
    let dir = "/etc/opt/chrome/policies/managed";
    let tree = FakeTree::default()
        .with(
            &format!("{dir}/a.json"),
            Ok(br#"{"HomepageLocation": "x"}"#),
        )
        .with(
            &format!("{dir}/b.json"),
            Ok(br#"{"RemoteDebuggingAllowed": false}"#),
        )
        .with(
            &format!("{dir}/c.txt"),
            Err(io::ErrorKind::PermissionDenied),
        );
    let read = read_linux(&tree);
    assert_eq!(read.found.len(), 4);
    assert_eq!(
        read.turned_off_by(),
        Some(place(Browser::Chrome, Scope::Machine))
    );
    assert_eq!(read.unreadable().count(), 0);
}

#[test]
fn past_either_linux_cap_is_a_read_error() {
    let dir = "/etc/chromium/policies/managed";
    let mut many = FakeTree::default();
    for at_file in 0..=MAX_FILES {
        many = many.with(&format!("{dir}/{at_file:03}.json"), Ok(b"{}"));
    }
    let read = read_linux(&many);
    let unreadable: Vec<_> = read.unreadable().collect();
    assert_eq!(
        unreadable,
        [(
            place(Browser::Chromium, Scope::Machine),
            "the folder holds more than 256 policy files"
        )]
    );
    // Exactly at the cap is read.
    let mut enough = FakeTree::default();
    for at_file in 0..MAX_FILES {
        enough = enough.with(&format!("{dir}/{at_file:03}.json"), Ok(b"{}"));
    }
    assert_eq!(read_linux(&enough).unreadable().count(), 0);

    let mut big = br#"{"RemoteDebuggingAllowed": true, "pad": ""#.to_vec();
    big.resize(usize::try_from(MAX_FILE_BYTES).unwrap(), b' ');
    big.extend_from_slice(br#""}"#);
    let tree = FakeTree::default().with(&format!("{dir}/big.json"), Ok(&big));
    let said = at(&read_linux(&tree), Browser::Chromium, Scope::Machine);
    assert_eq!(
        said,
        Found::Unreadable("a file is larger than 1 MiB".into())
    );
    let mut fits = br#"{"RemoteDebuggingAllowed": false}"#.to_vec();
    fits.resize(usize::try_from(MAX_FILE_BYTES).unwrap(), b' ');
    let tree = FakeTree::default().with(&format!("{dir}/fits.json"), Ok(&fits));
    let said = at(&read_linux(&tree), Browser::Chromium, Scope::Machine);
    assert_eq!(said, Found::Disallowed);
}
