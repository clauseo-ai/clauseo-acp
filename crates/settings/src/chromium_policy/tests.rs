//! Each reader over fakes: every place that says false turns remote debugging off, and a missing
//! value or an unreadable store does not.

use super::{
    Browser, Found, Hive, Place, PolicyTree, Registry, RemoteDebugging, Scope, json_says,
    plist_says, read_linux, read_managed_prefs, read_registry,
};
use crate::policy::registry::RegValue;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

const CHROME_KEY: &str = r"SOFTWARE\Policies\Google\Chrome";
const EDGE_KEY: &str = r"SOFTWARE\Policies\Microsoft\Edge";

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
    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        match self.files.get(path) {
            None => Ok(None),
            Some(Ok(bytes)) => Ok(Some(bytes.clone())),
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

/// Every place in `read` says absent.
fn all_absent(read: &RemoteDebugging) -> bool {
    read.found.iter().all(|(_, found)| *found == Found::Absent)
}

fn plist(value: Option<plist::Value>) -> Vec<u8> {
    let mut dict = plist::Dictionary::new();
    dict.insert("HomepageLocation".to_owned(), "https://firm.example".into());
    if let Some(value) = value {
        dict.insert(super::VALUE.to_owned(), value);
    }
    let mut bytes = Vec::new();
    plist::Value::Dictionary(dict)
        .to_writer_xml(&mut bytes)
        .unwrap();
    bytes
}

#[test]
fn each_registry_hive_and_browser_turns_it_off_at_zero() {
    for (hive, scope) in [
        (Hive::LocalMachine, Scope::Machine),
        (Hive::CurrentUser, Scope::User),
    ] {
        for (key, browser) in [(CHROME_KEY, Browser::Chrome), (EDGE_KEY, Browser::Edge)] {
            let registry = FakeRegistry::default().with(hive, key, Ok(RegValue::Dword(0)));
            let read = read_registry(&registry);
            assert_eq!(read.found.len(), 4);
            assert_eq!(read.turned_off_by(), Some(place(browser, scope)), "{key}");
            let registry = FakeRegistry::default().with(hive, key, Ok(RegValue::Dword(1)));
            assert_eq!(read_registry(&registry).turned_off_by(), None, "{key}");
        }
    }
}

#[test]
fn a_missing_or_unreadable_registry_value_does_not_turn_it_off() {
    let read = read_registry(&FakeRegistry::default());
    assert!(all_absent(&read));
    assert_eq!(read.turned_off_by(), None);
    let registry = FakeRegistry::default()
        .with(Hive::LocalMachine, CHROME_KEY, Err(5))
        .with(
            Hive::CurrentUser,
            EDGE_KEY,
            Ok(RegValue::Sz("0".to_owned())),
        );
    let read = read_registry(&registry);
    assert_eq!(read.turned_off_by(), None);
    let unreadable: Vec<Place> = read
        .found
        .iter()
        .filter(|(_, found)| matches!(found, Found::Unreadable(_)))
        .map(|(place, _)| *place)
        .collect();
    assert_eq!(
        unreadable,
        [
            place(Browser::Chrome, Scope::Machine),
            place(Browser::Edge, Scope::User)
        ]
    );
}

#[test]
fn each_managed_preferences_file_turns_it_off_when_false() {
    let user = || Ok(Some("lawyer".to_owned()));
    for (path, browser, scope) in [
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
            "/Library/Managed Preferences/lawyer/com.google.Chrome.plist",
            Browser::Chrome,
            Scope::User,
        ),
        (
            "/Library/Managed Preferences/lawyer/com.microsoft.Edge.plist",
            Browser::Edge,
            Scope::User,
        ),
    ] {
        let off = plist(Some(false.into()));
        let tree = FakeTree::default().with(path, Ok(&off));
        let read = read_managed_prefs(&tree, user());
        assert_eq!(read.found.len(), 4);
        assert_eq!(read.turned_off_by(), Some(place(browser, scope)), "{path}");
        let on = plist(Some(true.into()));
        let tree = FakeTree::default().with(path, Ok(&on));
        assert_eq!(read_managed_prefs(&tree, user()).turned_off_by(), None);
    }
}

#[test]
fn missing_or_unreadable_managed_preferences_do_not_turn_it_off() {
    let read = read_managed_prefs(&FakeTree::default(), Ok(Some("lawyer".to_owned())));
    assert!(all_absent(&read));
    let unset = plist(None);
    let tree = FakeTree::default()
        .with(
            "/Library/Managed Preferences/com.google.Chrome.plist",
            Ok(&unset),
        )
        .with(
            "/Library/Managed Preferences/com.microsoft.Edge.plist",
            Err(io::ErrorKind::PermissionDenied),
        )
        .with(
            "/Library/Managed Preferences/lawyer/com.google.Chrome.plist",
            Ok(b"not a plist"),
        );
    let read = read_managed_prefs(&tree, Ok(Some("lawyer".to_owned())));
    assert_eq!(read.turned_off_by(), None);
    assert_eq!(read.found[0].1, Found::Absent);
    assert!(matches!(read.found[1].1, Found::Unreadable(_)));
    assert!(matches!(read.found[2].1, Found::Unreadable(_)));
    // No login user: the user's files are unreadable, the computer's still count.
    let off = plist(Some(false.into()));
    let tree = FakeTree::default().with(
        "/Library/Managed Preferences/com.microsoft.Edge.plist",
        Ok(&off),
    );
    let read = read_managed_prefs(&tree, Ok(None));
    assert_eq!(
        read.turned_off_by(),
        Some(place(Browser::Edge, Scope::Machine))
    );
    assert!(matches!(read.found[2].1, Found::Unreadable(_)));
    let read = read_managed_prefs(&FakeTree::default(), Ok(Some("..".to_owned())));
    assert!(matches!(read.found[3].1, Found::Unreadable(_)));
}

#[test]
fn each_linux_managed_folder_turns_it_off_when_a_file_says_false() {
    for (path, browser) in [
        (
            "/etc/opt/chrome/policies/managed/firm.json",
            Browser::Chrome,
        ),
        (
            "/etc/opt/microsoft/msedge/policies/managed/firm.json",
            Browser::Edge,
        ),
    ] {
        let tree = FakeTree::default()
            .with(path, Ok(br#"{"RemoteDebuggingAllowed": false}"#))
            .with(
                "/etc/opt/chrome/policies/managed/other.json",
                Ok(br#"{"HomepageLocation": "https://firm.example"}"#),
            );
        let read = read_linux(&tree);
        assert_eq!(
            read.turned_off_by(),
            Some(place(browser, Scope::Machine)),
            "{path}"
        );
    }
    // Chrome does not take this policy as a recommendation, so neither does Clauseo.
    let tree = FakeTree::default().with(
        "/etc/opt/chrome/policies/recommended/firm.json",
        Ok(br#"{"RemoteDebuggingAllowed": false}"#),
    );
    assert_eq!(read_linux(&tree).turned_off_by(), None);
}

#[test]
fn missing_or_unreadable_linux_policy_does_not_turn_it_off() {
    let read = read_linux(&FakeTree::default());
    assert!(all_absent(&read));
    assert_eq!(read.found.len(), 2);
    let mut tree = FakeTree::default()
        .with("/etc/opt/chrome/policies/managed/a.json", Ok(b"{ not json"))
        .with(
            "/etc/opt/chrome/policies/managed/b.json",
            Err(io::ErrorKind::PermissionDenied),
        )
        .with(
            "/etc/opt/chrome/policies/managed/c.json",
            Ok(br#"{"RemoteDebuggingAllowed": 0}"#),
        );
    tree.broken_dirs
        .push(PathBuf::from("/etc/opt/microsoft/msedge/policies/managed"));
    let read = read_linux(&tree);
    assert_eq!(read.turned_off_by(), None);
    assert_eq!(read.found.len(), 4);
    assert!(
        read.found
            .iter()
            .all(|(_, found)| matches!(found, Found::Unreadable(_)))
    );
}

#[test]
fn the_parsers_read_only_a_boolean_and_name_no_value() {
    assert_eq!(
        json_says(br#"{"RemoteDebuggingAllowed": true}"#),
        Found::Allowed
    );
    assert_eq!(
        json_says(b"[]"),
        Found::Unreadable("a file is not a JSON object".into())
    );
    assert_eq!(
        plist_says(&plist(Some(0.into()))),
        Found::Unreadable("the value is not a boolean".into())
    );
    assert_eq!(plist_says(&plist(None)), Found::Absent);
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
}
