//! Chrome's, Edge's and Chromium's own `RemoteDebuggingAllowed` policy (and Brave's, on macOS), as
//! a firm's IT sets it for those browsers. Every browser Clauseo drives needs remote debugging
//! (agent-browser speaks CDP), and false turns it off. Chrome for Testing reads its own policy
//! location, and chrome-headless-shell none, so a browser Clauseo downloaded would not refuse by
//! itself. The host reads the firm's values here, before it picks any browser, and starts none
//! when one is false or a place cannot be read, unless Clauseo's own policy allows it
//! (`browser.allow_when_remote_debugging_disabled`).
//!
//! Where it is read, for the computer and the user, taking what Chrome's own policy conversion
//! takes as a boolean:
//! - Windows: `RemoteDebuggingAllowed` under `SOFTWARE\Policies\Google\Chrome`,
//!   `SOFTWARE\Policies\Microsoft\Edge` and `SOFTWARE\Policies\Chromium`, in `HKLM` and in
//!   `HKCU` (a `REG_DWORD`, or a `REG_SZ` holding an integer; 0 is false, any other true);
//!   and under the same keys `DeveloperToolsAvailability` ([`DEVTOOLS_VALUE`]), whose 2
//!   (developer tools disallowed) also turns it off, the computer's value over the user's;
//!   [`read_registry`] over a [`Registry`].
//! - macOS: the managed preferences files of `com.google.Chrome`, `com.microsoft.Edge`,
//!   `org.chromium.Chromium` and `com.brave.Browser`, the computer's and the login user's (a
//!   boolean, or the integer 0 or 1); [`read_managed_prefs`]. Brave only here: the host looks
//!   for Brave on macOS alone (`clauseo_host::browser::chrome`).
//! - Linux: every `*.json` in Chrome's, Edge's and Chromium's `managed` policy folders (a
//!   boolean, or the integer 0 or 1), at most [`MAX_FILES`] files of [`MAX_FILE_BYTES`] each
//!   (Chrome takes this policy only as mandatory, never as recommended: its definition does not
//!   set `can_be_recommended`); [`read_linux`].
//!
//! On every OS a `DeveloperToolsAvailability` of 2 turns developer tools off, and with them
//! remote debugging, so it counts as a false ([`DEVTOOLS_VALUE`], one reading of the number,
//! [`devtools_says`]): on Windows per browser, the computer's value over the user's; on macOS
//! and Linux in each file beside `RemoteDebuggingAllowed`, a false from either winning.
//!
//! Every store comes in through a trait ([`Registry`], [`PolicyTree`]), so the tests use fakes,
//! and every reader's logic is compiled and tested on every OS. Each place says one of three
//! things besides true or false: nothing set (the key, value, file or folder is missing,
//! [`Found::Absent`]); a bad value (read, but malformed or of a type Chrome does not take,
//! [`Found::BadValue`], ignored as Chrome ignores it); or a read error (anything else, past a cap
//! included, [`Found::Unreadable`]: nothing is known, so the host starts no browser). The
//! reasons name no path.

#[cfg(test)]
mod tests;

use crate::policy::managed_prefs;
use crate::policy::registry::RegValue;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// The policy's name in every store.
pub const VALUE: &str = "RemoteDebuggingAllowed";

/// Chrome's developer tools policy, read beside [`VALUE`] on every OS ([`devtools_says`]): 2
/// ([`DEVTOOLS_DISALLOWED`]) disallows the developer tools, and a browser so set will not let a
/// client attach to its pages, so it counts as [`Found::Disallowed`]; 0, 1 and 3 say nothing
/// about remote debugging, leaving it to [`VALUE`]; any other value is a bad one.
pub const DEVTOOLS_VALUE: &str = "DeveloperToolsAvailability";

/// [`DEVTOOLS_VALUE`]'s value that disallows the developer tools.
pub const DEVTOOLS_DISALLOWED: i64 = 2;

/// A browser whose policy Clauseo reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    /// Google Chrome.
    Chrome,
    /// Microsoft Edge.
    Edge,
    /// Chromium-branded builds (a forced browser path can point at one).
    Chromium,
    /// Brave, which the host looks for on macOS.
    Brave,
}

/// Whose policy: the computer's or the user's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The computer's (`HKLM`, the computer's managed preferences file, Linux's folders).
    Machine,
    /// The user's (`HKCU`, the login user's managed preferences file).
    User,
}

/// One place the policy is read from, named without a path (for logs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    /// Whose browser.
    pub browser: Browser,
    /// Whose policy.
    pub scope: Scope,
}

/// What one place said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// No value there.
    Absent,
    /// `RemoteDebuggingAllowed` true.
    Allowed,
    /// `RemoteDebuggingAllowed` false: remote debugging is off.
    Disallowed,
    /// A value was read but is malformed, or of a type Chrome does not take: ignored, as Chrome
    /// ignores it. Why, with no path or value.
    BadValue(String),
    /// The store could not be read (not missing: denied, failed, or past a cap), so nothing is
    /// known there. Why, with no path or value.
    Unreadable(String),
}

/// What every place said, in the order read. A place with several files (Linux) can appear
/// once per file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteDebugging {
    /// Each place and what it said.
    pub found: Vec<(Place, Found)>,
}

/// Reads the firm's Chrome, Edge and Chromium `RemoteDebuggingAllowed`. Blocking (registry or
/// files): call it off the host's actor.
pub trait RemoteDebuggingPolicy: Send + Sync + fmt::Debug {
    /// What every place says now.
    fn read(&self) -> RemoteDebugging;
}

/// This machine's stores: the registry on Windows, managed preferences files on macOS, the
/// policy folders on Linux; nothing elsewhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemPolicy;

/// A reader that finds nothing set anywhere: the host's default in its tests, so they never
/// read the machine they run on.
#[derive(Debug, Clone, Copy, Default)]
pub struct NothingSet;

/// A registry root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    /// `HKEY_LOCAL_MACHINE`.
    LocalMachine,
    /// `HKEY_CURRENT_USER`.
    CurrentUser,
}

/// The registry, as [`read_registry`] asks it.
pub trait Registry {
    /// The value `value` of `hive\key`, through the 64-bit view: `Ok(None)` when the key or
    /// the value is missing, `Err` with the Win32 error code when the read failed.
    fn value(&self, hive: Hive, key: &str, value: &str) -> Result<Option<RegValue>, u32>;
}

/// Files and folders, as [`read_managed_prefs`] and [`read_linux`] read them.
pub trait PolicyTree {
    /// The file's bytes, at most `max + 1` of them (so a longer file shows as longer than
    /// `max`); `Ok(None)` when it is missing.
    fn read(&self, path: &Path, max: u64) -> io::Result<Option<Vec<u8>>>;
    /// The `*.json` files in `dir` (folders left out), sorted; empty when the folder is
    /// missing. It may stop listing once it has more than [`MAX_FILES`].
    fn json_files(&self, dir: &Path) -> io::Result<Vec<PathBuf>>;
}

/// The most `*.json` files read from one Linux folder; more is a read error.
pub const MAX_FILES: usize = 256;

/// The most bytes read from one Linux policy file (16 MiB, room for a firm's long URL lists); a
/// longer file is a read error.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Whether a Win32 error code from a registry read means the key or value is missing:
/// `ERROR_FILE_NOT_FOUND` (2) or `ERROR_PATH_NOT_FOUND` (3). Any other is a read error.
pub fn registry_missing(code: u32) -> bool {
    matches!(code, 2 | 3)
}

/// Whether a file system error means nothing is there: the path is missing, or a file stands
/// where a folder on it should be (Chrome skips both). Any other is a read error.
fn fs_missing(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// The real file system, following links.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTree;

/// The registry keys, by browser.
pub const REGISTRY_KEYS: [(Browser, &str); 3] = [
    (Browser::Chrome, r"SOFTWARE\Policies\Google\Chrome"),
    (Browser::Edge, r"SOFTWARE\Policies\Microsoft\Edge"),
    (Browser::Chromium, r"SOFTWARE\Policies\Chromium"),
];

/// The managed preferences domains, by browser.
pub const MAC_DOMAINS: [(Browser, &str); 4] = [
    (Browser::Chrome, "com.google.Chrome"),
    (Browser::Edge, "com.microsoft.Edge"),
    (Browser::Chromium, "org.chromium.Chromium"),
    (Browser::Brave, "com.brave.Browser"),
];

/// The Linux folders of mandatory policy, by browser.
pub const LINUX_DIRS: [(Browser, &str); 3] = [
    (Browser::Chrome, "/etc/opt/chrome/policies/managed"),
    (Browser::Edge, "/etc/opt/edge/policies/managed"),
    (Browser::Chromium, "/etc/chromium/policies/managed"),
];

impl RemoteDebugging {
    /// The first place that turned remote debugging off, if any did.
    pub fn turned_off_by(&self) -> Option<Place> {
        self.found
            .iter()
            .find(|(_, found)| *found == Found::Disallowed)
            .map(|(place, _)| *place)
    }

    /// Each place that could not be read, with why.
    pub fn unreadable(&self) -> impl Iterator<Item = (Place, &str)> {
        self.found.iter().filter_map(|(place, found)| match found {
            Found::Unreadable(why) => Some((*place, why.as_str())),
            _ => None,
        })
    }

    /// Each place that holds a bad value, with why.
    pub fn bad_values(&self) -> impl Iterator<Item = (Place, &str)> {
        self.found.iter().filter_map(|(place, found)| match found {
            Found::BadValue(why) => Some((*place, why.as_str())),
            _ => None,
        })
    }
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let browser = match self.browser {
            Browser::Chrome => "Chrome",
            Browser::Edge => "Edge",
            Browser::Chromium => "Chromium",
            Browser::Brave => "Brave",
        };
        let scope = match self.scope {
            Scope::Machine => "computer",
            Scope::User => "user",
        };
        write!(f, "{browser}'s {scope} policy")
    }
}

impl RemoteDebuggingPolicy for SystemPolicy {
    fn read(&self) -> RemoteDebugging {
        #[cfg(windows)]
        return read_registry(&crate::policy::SystemRegistry);
        #[cfg(target_os = "macos")]
        return read_managed_prefs(&SystemTree, managed_prefs::login_user());
        #[cfg(target_os = "linux")]
        return read_linux(&SystemTree);
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        return RemoteDebugging::default();
    }
}

impl RemoteDebuggingPolicy for NothingSet {
    fn read(&self) -> RemoteDebugging {
        RemoteDebugging::default()
    }
}

impl PolicyTree for SystemTree {
    fn read(&self, path: &Path, max: u64) -> io::Result<Option<Vec<u8>>> {
        use std::io::Read as _;
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if fs_missing(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(max.saturating_add(1)).read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }

    fn json_files(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if fs_missing(&error) => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            // Chrome lists files only; a link is followed when it is read.
            if path.extension().is_some_and(|ext| ext == "json") && !entry.file_type()?.is_dir() {
                files.push(path);
            }
            if files.len() > MAX_FILES {
                break;
            }
        }
        files.sort();
        Ok(files)
    }
}

/// The six registry places: `HKLM` then `HKCU`, Chrome, Edge, then Chromium in each; then
/// each browser's `DeveloperToolsAvailability` ([`devtools_from_registry`]).
pub fn read_registry(registry: &dyn Registry) -> RemoteDebugging {
    let mut found = Vec::new();
    for (hive, scope) in [
        (Hive::LocalMachine, Scope::Machine),
        (Hive::CurrentUser, Scope::User),
    ] {
        for (browser, key) in REGISTRY_KEYS {
            let said = match registry.value(hive, key, VALUE) {
                Ok(None) => Found::Absent,
                Ok(Some(value)) => registry_says(&value),
                Err(code) => Found::Unreadable(format!("the registry read failed ({code})")),
            };
            found.push((Place { browser, scope }, said));
        }
    }
    for (browser, key) in REGISTRY_KEYS {
        found.extend(devtools_from_registry(registry, browser, key));
    }
    RemoteDebugging { found }
}

/// One browser's `DeveloperToolsAvailability` ([`devtools_says`]), the computer's value over
/// the user's as Chrome merges its policies: the computer's when it holds a good one, else the
/// user's. The place that holds 2 says [`Found::Disallowed`]; nothing set, 0, 1 or 3 adds
/// nothing; a failed read says [`Found::Unreadable`] (the user's is still read when the
/// computer's failed, since a 2 there blocks whatever the computer holds); a value Chrome would
/// not take says [`Found::BadValue`] and, ignored as Chrome ignores it, leaves it to the user's.
fn devtools_from_registry(
    registry: &dyn Registry,
    browser: Browser,
    key: &str,
) -> Vec<(Place, Found)> {
    let mut found = Vec::new();
    for (hive, scope) in [
        (Hive::LocalMachine, Scope::Machine),
        (Hive::CurrentUser, Scope::User),
    ] {
        let place = Place { browser, scope };
        match registry.value(hive, key, DEVTOOLS_VALUE) {
            Ok(None) => {}
            Ok(Some(value)) => {
                let said = match registry_integer(&value) {
                    Ok(number) => devtools_says(Some(number)),
                    Err(why) => Found::BadValue(why.to_owned()),
                };
                match said {
                    bad @ Found::BadValue(_) => found.push((place, bad)),
                    // A good value set here decides it: the user's is not read.
                    Found::Disallowed => {
                        found.push((place, Found::Disallowed));
                        return found;
                    }
                    Found::Absent | Found::Allowed | Found::Unreadable(_) => return found,
                }
            }
            Err(code) => found.push((
                place,
                Found::Unreadable(format!("the registry read failed ({code})")),
            )),
        }
    }
    found
}

/// A registry value as an integer, as Chrome converts it: a `REG_DWORD`, or a `REG_SZ` holding
/// an integer (spaces around it allowed).
fn registry_integer(value: &RegValue) -> Result<i64, &'static str> {
    match value {
        RegValue::Dword(number) => Ok(i64::from(*number)),
        RegValue::Sz(text) => text
            .trim()
            .parse::<i64>()
            .ok()
            .ok_or("the REG_SZ is not an integer"),
        RegValue::MultiSz(_) | RegValue::Other(_) => {
            Err("the value is neither a REG_DWORD nor a REG_SZ")
        }
    }
}

/// What a registry value says, as Chrome converts it to a boolean: a `REG_DWORD`, or a `REG_SZ`
/// holding an integer (spaces around it allowed); 0 is false, any other number true.
pub fn registry_says(value: &RegValue) -> Found {
    match registry_integer(value) {
        Ok(0) => Found::Disallowed,
        Ok(_) => Found::Allowed,
        Err(why) => Found::BadValue(why.to_owned()),
    }
}

/// The eight macOS places: the computer's file then the user's, Chrome, Edge, Chromium, then
/// Brave in each.
/// `looked_up` is the login user ([`managed_prefs::login_user`]); without one, the user's
/// file cannot be found, so the user's places are read errors.
pub fn read_managed_prefs(
    tree: &dyn PolicyTree,
    looked_up: io::Result<Option<String>>,
) -> RemoteDebugging {
    let user = match looked_up {
        Ok(Some(user)) => Ok(user),
        Ok(None) => Err("the real user id has no user".to_owned()),
        Err(error) => Err(format!("the login user was not found ({})", error.kind())),
    };
    let mut found = Vec::new();
    for scope in [Scope::Machine, Scope::User] {
        for (browser, domain) in MAC_DOMAINS {
            let path = match (&user, scope) {
                (_, Scope::Machine) => {
                    Ok(Path::new(managed_prefs::MANAGED_DIR).join(format!("{domain}.plist")))
                }
                // `file_paths` gives the computer's file, then the user's.
                (Ok(user), Scope::User) => managed_prefs::file_paths(domain, user)
                    .ok()
                    .and_then(|paths| paths.into_iter().nth(1))
                    .ok_or_else(|| "the user name is no plain folder name".to_owned()),
                (Err(why), Scope::User) => Err(why.clone()),
            };
            let said = match path {
                Ok(path) => plist_file(tree, &path),
                Err(why) => Found::Unreadable(why),
            };
            found.push((Place { browser, scope }, said));
        }
    }
    RemoteDebugging { found }
}

/// What one managed preferences file says.
fn plist_file(tree: &dyn PolicyTree, path: &Path) -> Found {
    match tree.read(path, u64::MAX) {
        Ok(None) => Found::Absent,
        Ok(Some(bytes)) => plist_says(&bytes),
        Err(error) => Found::Unreadable(format!("the file was not read ({})", error.kind())),
    }
}

/// What a property list's `RemoteDebuggingAllowed` and `DeveloperToolsAvailability` say
/// ([`both`]).
pub fn plist_says(bytes: &[u8]) -> Found {
    let Some(dict) = plist::Value::from_reader(io::Cursor::new(bytes))
        .ok()
        .and_then(plist::Value::into_dictionary)
    else {
        return Found::BadValue("not a property list dictionary".to_owned());
    };
    let remote = match dict.get(VALUE) {
        None => Found::Absent,
        Some(plist::Value::Boolean(false)) => Found::Disallowed,
        Some(plist::Value::Boolean(true)) => Found::Allowed,
        Some(plist::Value::Integer(number)) => zero_or_one(number.as_signed()),
        Some(_) => Found::BadValue(NOT_A_BOOLEAN.to_owned()),
    };
    let devtools = match dict.get(DEVTOOLS_VALUE) {
        None => Found::Absent,
        Some(plist::Value::Integer(number)) => devtools_says(number.as_signed()),
        Some(_) => Found::BadValue(NOT_DEVTOOLS.to_owned()),
    };
    both(remote, devtools)
}

/// Why a `DeveloperToolsAvailability` is bad when it is no integer from 0 to 3.
const NOT_DEVTOOLS: &str = "the developer tools value is not 0, 1, 2 or 3";

/// What a `DeveloperToolsAvailability` number says, in every store (the registry, a property
/// list, a JSON file): [`DEVTOOLS_DISALLOWED`] is a false; 0, 1 and 3 say nothing of remote
/// debugging; any other number, or one that is no integer (`None`), is a bad value.
pub fn devtools_says(number: Option<i64>) -> Found {
    match number {
        Some(DEVTOOLS_DISALLOWED) => Found::Disallowed,
        Some(0 | 1 | 3) => Found::Absent,
        _ => Found::BadValue(NOT_DEVTOOLS.to_owned()),
    }
}

/// One file's word from its two values: a false from either wins; otherwise
/// `RemoteDebuggingAllowed`'s, unless that says nothing and the developer tools value is bad.
fn both(remote: Found, devtools: Found) -> Found {
    match (remote, devtools) {
        (_, Found::Disallowed) => Found::Disallowed,
        (Found::Absent, bad @ Found::BadValue(_)) => bad,
        (remote, _) => remote,
    }
}

/// Why a value is bad when it is neither a boolean nor the integer 0 or 1.
const NOT_A_BOOLEAN: &str = "the value is not a boolean, 0 or 1";

/// The integer 0 as false and 1 as true; any other is a bad value.
fn zero_or_one(number: Option<i64>) -> Found {
    match number {
        Some(0) => Found::Disallowed,
        Some(1) => Found::Allowed,
        _ => Found::BadValue(NOT_A_BOOLEAN.to_owned()),
    }
}

/// The Linux places: each `*.json` in Chrome's, Edge's, then Chromium's `managed` folder,
/// one entry per file; a folder with no file says [`Found::Absent`] once. A folder of more
/// than [`MAX_FILES`] files, or a file of more than [`MAX_FILE_BYTES`], is a read error.
pub fn read_linux(tree: &dyn PolicyTree) -> RemoteDebugging {
    let mut found = Vec::new();
    for (browser, dir) in LINUX_DIRS {
        let place = Place {
            browser,
            scope: Scope::Machine,
        };
        let files = match tree.json_files(Path::new(dir)) {
            Ok(files) if files.len() > MAX_FILES => {
                let why = format!("the folder holds more than {MAX_FILES} policy files");
                found.push((place, Found::Unreadable(why)));
                continue;
            }
            Ok(files) => files,
            Err(error) => {
                let why = format!("the folder was not read ({})", error.kind());
                found.push((place, Found::Unreadable(why)));
                continue;
            }
        };
        if files.is_empty() {
            found.push((place, Found::Absent));
        }
        for file in files {
            let said = match tree.read(&file, MAX_FILE_BYTES) {
                Ok(None) => Found::Absent,
                Ok(Some(bytes)) if bytes.len() as u64 > MAX_FILE_BYTES => {
                    Found::Unreadable("a file is larger than 16 MiB".to_owned())
                }
                Ok(Some(bytes)) => json_says(&bytes),
                Err(error) => Found::Unreadable(format!("a file was not read ({})", error.kind())),
            };
            found.push((place, said));
        }
    }
    RemoteDebugging { found }
}

/// What a JSON policy file's `RemoteDebuggingAllowed` and `DeveloperToolsAvailability` say
/// ([`both`]).
pub fn json_says(bytes: &[u8]) -> Found {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_slice(bytes) else {
        return Found::BadValue("a file is not a JSON object".to_owned());
    };
    let remote = match map.get(VALUE) {
        None => Found::Absent,
        Some(serde_json::Value::Bool(false)) => Found::Disallowed,
        Some(serde_json::Value::Bool(true)) => Found::Allowed,
        Some(serde_json::Value::Number(number)) => zero_or_one(number.as_i64()),
        Some(_) => Found::BadValue(NOT_A_BOOLEAN.to_owned()),
    };
    let devtools = match map.get(DEVTOOLS_VALUE) {
        None => Found::Absent,
        Some(serde_json::Value::Number(number)) => devtools_says(number.as_i64()),
        Some(_) => Found::BadValue(NOT_DEVTOOLS.to_owned()),
    };
    both(remote, devtools)
}
