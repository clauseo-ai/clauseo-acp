//! Each reader over fakes, for Chrome, Edge and Chromium: nothing set, false, true, a bad value
//! and a read error each say what they are, and only false turns remote debugging off. The
//! macOS and Linux readers are in `tests/files.rs`.

mod files;

use super::{
    Browser, Found, Hive, Place, PolicyTree, Registry, RemoteDebugging, Scope, json_says,
    plist_says, read_registry, registry_says,
};
use crate::policy::registry::RegValue;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

const KEYS: [(&str, Browser); 3] = [
    (r"SOFTWARE\Policies\Google\Chrome", Browser::Chrome),
    (r"SOFTWARE\Policies\Microsoft\Edge", Browser::Edge),
    (r"SOFTWARE\Policies\Chromium", Browser::Chromium),
];

/// A registry of the values given; any other is missing.
#[derive(Default)]
struct FakeRegistry(BTreeMap<(u8, String, String), Result<RegValue, u32>>);

impl FakeRegistry {
    fn with(mut self, hive: Hive, key: &str, value: Result<RegValue, u32>) -> Self {
        self.0.insert(
            (hive_id(hive), key.to_owned(), super::VALUE.to_owned()),
            value,
        );
        self
    }
}

fn hive_id(hive: Hive) -> u8 {
    match hive {
        Hive::LocalMachine => 0,
        Hive::CurrentUser => 1,
    }
}

impl Registry for FakeRegistry {
    fn value(&self, hive: Hive, key: &str, value: &str) -> Result<Option<RegValue>, u32> {
        match self
            .0
            .get(&(hive_id(hive), key.to_owned(), value.to_owned()))
        {
            None => Ok(None),
            Some(Ok(found)) => Ok(Some(found.clone())),
            Some(Err(code)) => Err(*code),
        }
    }
}

/// Files of the bytes given, or that fail to read; folders listed from them.
#[derive(Default)]
struct FakeTree {
    files: BTreeMap<PathBuf, Result<Vec<u8>, io::ErrorKind>>,
    broken_dirs: Vec<PathBuf>,
}

impl FakeTree {
    fn with(mut self, path: &str, bytes: Result<&[u8], io::ErrorKind>) -> Self {
        self.files
            .insert(PathBuf::from(path), bytes.map(<[u8]>::to_vec));
        self
    }
}

impl PolicyTree for FakeTree {
    fn read(&self, path: &Path, max: u64) -> io::Result<Option<Vec<u8>>> {
        let keep = usize::try_from(max.saturating_add(1)).unwrap_or(usize::MAX);
        match self.files.get(path) {
            None => Ok(None),
            Some(Ok(bytes)) => Ok(Some(bytes.iter().copied().take(keep).collect())),
            Some(Err(kind)) => Err(io::Error::from(*kind)),
        }
    }

    fn json_files(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        if self.broken_dirs.iter().any(|broken| broken == dir) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok(self
            .files
            .keys()
            .filter(|path| path.parent() == Some(dir))
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .cloned()
            .collect())
    }
}

fn place(browser: Browser, scope: Scope) -> Place {
    Place { browser, scope }
}

/// What `read` found at the one entry for `browser` and `scope`.
fn at(read: &RemoteDebugging, browser: Browser, scope: Scope) -> Found {
    read.found
        .iter()
        .find(|(found_at, _)| *found_at == place(browser, scope))
        .map(|(_, found)| found.clone())
        .expect("the place was read")
}

/// Every place in `read` but `browser`'s at `scope` says absent.
fn others_absent(read: &RemoteDebugging, browser: Browser, scope: Scope) -> bool {
    read.found
        .iter()
        .filter(|(found_at, _)| *found_at != place(browser, scope))
        .all(|(_, found)| *found == Found::Absent)
}

/// The kind of what a place said, without its reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Absent,
    Allowed,
    Disallowed,
    Bad,
    Unreadable,
}

fn kind(found: &Found) -> Kind {
    match found {
        Found::Absent => Kind::Absent,
        Found::Allowed => Kind::Allowed,
        Found::Disallowed => Kind::Disallowed,
        Found::BadValue(_) => Kind::Bad,
        Found::Unreadable(_) => Kind::Unreadable,
    }
}

#[test]
fn each_registry_place_says_nothing_false_true_bad_or_unreadable() {
    let cases: [(Result<RegValue, u32>, Kind); 11] = [
        (Ok(RegValue::Dword(0)), Kind::Disallowed),
        (Ok(RegValue::Dword(1)), Kind::Allowed),
        (Ok(RegValue::Dword(7)), Kind::Allowed),
        (Ok(RegValue::Sz("0".to_owned())), Kind::Disallowed),
        (Ok(RegValue::Sz(" 0 ".to_owned())), Kind::Disallowed),
        (Ok(RegValue::Sz("1".to_owned())), Kind::Allowed),
        (Ok(RegValue::Sz("no".to_owned())), Kind::Bad),
        (Ok(RegValue::Sz(String::new())), Kind::Bad),
        (Ok(RegValue::Other(3)), Kind::Bad),
        (Ok(RegValue::MultiSz(vec!["0".to_owned()])), Kind::Bad),
        (Err(5), Kind::Unreadable),
    ];
    for (hive, scope) in [
        (Hive::LocalMachine, Scope::Machine),
        (Hive::CurrentUser, Scope::User),
    ] {
        for (key, browser) in KEYS {
            for (value, expected) in &cases {
                let registry = FakeRegistry::default().with(hive, key, value.clone());
                let read = read_registry(&registry);
                assert_eq!(read.found.len(), 6);
                let said = at(&read, browser, scope);
                assert_eq!(kind(&said), *expected, "{key} {value:?}");
                assert!(others_absent(&read, browser, scope), "{key} {value:?}");
                let off = (*expected == Kind::Disallowed).then_some(place(browser, scope));
                assert_eq!(read.turned_off_by(), off, "{key} {value:?}");
            }
        }
    }
    let read = read_registry(&FakeRegistry::default());
    assert!(read.found.iter().all(|(_, found)| *found == Found::Absent));
}

// A missing key or value, or a missing path on the way to it, is nothing set; any other error
// is a read error.
#[test]
fn registry_file_and_path_not_found_are_missing() {
    assert!(super::registry_missing(2)); // ERROR_FILE_NOT_FOUND
    assert!(super::registry_missing(3)); // ERROR_PATH_NOT_FOUND
    for code in [0, 5, 1018] {
        assert!(!super::registry_missing(code), "{code}");
    }
}

#[test]
fn the_registry_reasons_name_no_value_or_code_beyond_the_error() {
    assert_eq!(
        registry_says(&RegValue::Sz("no".to_owned())),
        Found::BadValue("the REG_SZ is not an integer".into())
    );
    let registry = FakeRegistry::default().with(Hive::LocalMachine, KEYS[0].0, Err(5));
    let read = read_registry(&registry);
    let unreadable: Vec<_> = read.unreadable().collect();
    assert_eq!(
        unreadable,
        [(
            place(Browser::Chrome, Scope::Machine),
            "the registry read failed (5)"
        )]
    );
    assert_eq!(read.bad_values().count(), 0);
}

#[test]
fn the_parsers_take_a_boolean_or_0_or_1_and_name_no_value() {
    assert_eq!(
        json_says(br#"{"RemoteDebuggingAllowed": true}"#),
        Found::Allowed
    );
    assert_eq!(
        json_says(br#"{"RemoteDebuggingAllowed": 0}"#),
        Found::Disallowed
    );
    assert_eq!(
        json_says(br#"{"RemoteDebuggingAllowed": 1}"#),
        Found::Allowed
    );
    for bad in [
        &br#"{"RemoteDebuggingAllowed": 2}"#[..],
        br#"{"RemoteDebuggingAllowed": 0.0}"#,
        br#"{"RemoteDebuggingAllowed": "false"}"#,
    ] {
        assert_eq!(
            json_says(bad),
            Found::BadValue("the value is not a boolean, 0 or 1".into())
        );
    }
    assert_eq!(
        json_says(b"[]"),
        Found::BadValue("a file is not a JSON object".into())
    );
    assert_eq!(plist_says(&files::plist(Some(0.into()))), Found::Disallowed);
    assert_eq!(plist_says(&files::plist(Some(1.into()))), Found::Allowed);
    assert_eq!(
        plist_says(&files::plist(Some((-1).into()))),
        Found::BadValue("the value is not a boolean, 0 or 1".into())
    );
    assert_eq!(plist_says(&files::plist(None)), Found::Absent);
}

#[test]
fn a_place_is_named_without_a_path() {
    assert_eq!(
        place(Browser::Chrome, Scope::Machine).to_string(),
        "Chrome's computer policy"
    );
    assert_eq!(
        place(Browser::Edge, Scope::User).to_string(),
        "Edge's user policy"
    );
    assert_eq!(
        place(Browser::Chromium, Scope::User).to_string(),
        "Chromium's user policy"
    );
}
