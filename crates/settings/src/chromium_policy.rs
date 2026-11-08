//! Chrome's and Edge's own `RemoteDebuggingAllowed` policy, as a firm's IT sets it for those
//! browsers. Every browser Clauseo drives needs remote debugging (agent-browser speaks CDP), and
//! false turns it off. Chrome for Testing reads its own policy location, and
//! chrome-headless-shell none, so a browser Clauseo downloaded would not refuse by itself. The
//! host reads the firm's Chrome and Edge values here, before it picks any browser, and starts
//! none when one is false, unless Clauseo's own policy allows it
//! (`browser.allow_when_remote_debugging_disabled`).
//!
//! Where it is read, for the computer and the user:
//! - Windows: `RemoteDebuggingAllowed` under `SOFTWARE\Policies\Google\Chrome` and
//!   `SOFTWARE\Policies\Microsoft\Edge`, in `HKLM` and in `HKCU` (a `REG_DWORD`, 0 is false);
//!   [`read_registry`] over a [`Registry`].
//! - macOS: the managed preferences files of `com.google.Chrome` and `com.microsoft.Edge`, the
//!   computer's and the login user's (a boolean); [`read_managed_prefs`].
//! - Linux: every `*.json` in Chrome's and Edge's `managed` policy folders (Chrome takes this
//!   policy only as mandatory, never as recommended: its definition does not set
//!   `can_be_recommended`); [`read_linux`].
//!
//! Every store comes in through a trait ([`Registry`], [`PolicyTree`]), so the tests use fakes,
//! and every reader's logic is compiled and tested on every OS. A value that is missing, or a
//! store that cannot be read, never turns the browser off: it is reported as
//! [`Found::Absent`] or [`Found::Unreadable`], whose reason names no path.

#[cfg(test)]
mod tests;

use crate::policy::managed_prefs;
use crate::policy::registry::RegValue;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// The policy's name in every store.
pub const VALUE: &str = "RemoteDebuggingAllowed";

/// A browser whose policy Clauseo reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    /// Google Chrome.
    Chrome,
    /// Microsoft Edge.
    Edge,
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
    /// The store could not be read, or holds a value of another type. Why, with no path or
    /// value.
    Unreadable(String),
}

/// What every place said, in the order read. A place with several files (Linux) can appear
/// once per file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteDebugging {
    /// Each place and what it said.
    pub found: Vec<(Place, Found)>,
}

/// Reads the firm's Chrome and Edge `RemoteDebuggingAllowed`. Blocking (registry or files):
/// call it off the host's actor.
pub trait RemoteDebuggingPolicy: Send + Sync + fmt::Debug {
    /// What every place says now.
    fn read(&self) -> RemoteDebugging;
}

/// This machine's stores: the registry on Windows, managed preferences files on macOS, the
/// policy folders on Linux; nothing elsewhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemPolicy;

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
    /// The file's bytes; `Ok(None)` when it is missing.
    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>>;
    /// The `*.json` files in `dir`, sorted; empty when the folder is missing.
    fn json_files(&self, dir: &Path) -> io::Result<Vec<PathBuf>>;
}

/// The real file system, following links.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTree;

/// The registry keys, by browser.
pub const REGISTRY_KEYS: [(Browser, &str); 2] = [
    (Browser::Chrome, r"SOFTWARE\Policies\Google\Chrome"),
    (Browser::Edge, r"SOFTWARE\Policies\Microsoft\Edge"),
];

/// The managed preferences domains, by browser.
pub const MAC_DOMAINS: [(Browser, &str); 2] = [
    (Browser::Chrome, "com.google.Chrome"),
    (Browser::Edge, "com.microsoft.Edge"),
];

/// The Linux folders of mandatory policy, by browser.
pub const LINUX_DIRS: [(Browser, &str); 2] = [
    (Browser::Chrome, "/etc/opt/chrome/policies/managed"),
    (Browser::Edge, "/etc/opt/microsoft/msedge/policies/managed"),
];

impl RemoteDebugging {
    /// The first place that turned remote debugging off, if any did.
    pub fn turned_off_by(&self) -> Option<Place> {
        self.found
            .iter()
            .find(|(_, found)| *found == Found::Disallowed)
            .map(|(place, _)| *place)
    }
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let browser = match self.browser {
            Browser::Chrome => "Chrome",
            Browser::Edge => "Edge",
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

impl PolicyTree for SystemTree {
    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn json_files(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut files = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                files.push(path);
            }
        }
        files.sort();
        Ok(files)
    }
}

/// The four registry places: `HKLM` then `HKCU`, Chrome then Edge in each.
pub fn read_registry(registry: &dyn Registry) -> RemoteDebugging {
    let mut found = Vec::new();
    for (hive, scope) in [
        (Hive::LocalMachine, Scope::Machine),
        (Hive::CurrentUser, Scope::User),
    ] {
        for (browser, key) in REGISTRY_KEYS {
            let said = match registry.value(hive, key, VALUE) {
                Ok(None) => Found::Absent,
                Ok(Some(RegValue::Dword(0))) => Found::Disallowed,
                Ok(Some(RegValue::Dword(_))) => Found::Allowed,
                Ok(Some(_)) => Found::Unreadable("the value is not a REG_DWORD".to_owned()),
                Err(code) => Found::Unreadable(format!("the registry read failed ({code})")),
            };
            found.push((Place { browser, scope }, said));
        }
    }
    RemoteDebugging { found }
}

/// The four macOS places: the computer's file then the user's, Chrome then Edge in each.
/// `looked_up` is the login user ([`managed_prefs::login_user`]); without one, the user's
/// places are unreadable.
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
    match tree.read(path) {
        Ok(None) => Found::Absent,
        Ok(Some(bytes)) => plist_says(&bytes),
        Err(error) => Found::Unreadable(format!("the file was not read ({})", error.kind())),
    }
}

/// What a property list's `RemoteDebuggingAllowed` says.
pub fn plist_says(bytes: &[u8]) -> Found {
    let Some(dict) = plist::Value::from_reader(io::Cursor::new(bytes))
        .ok()
        .and_then(plist::Value::into_dictionary)
    else {
        return Found::Unreadable("not a property list dictionary".to_owned());
    };
    match dict.get(VALUE) {
        None => Found::Absent,
        Some(plist::Value::Boolean(false)) => Found::Disallowed,
        Some(plist::Value::Boolean(true)) => Found::Allowed,
        Some(_) => Found::Unreadable("the value is not a boolean".to_owned()),
    }
}

/// The Linux places: each `*.json` in Chrome's then Edge's `managed` folder, one entry per
/// file; a folder with no file says [`Found::Absent`] once.
pub fn read_linux(tree: &dyn PolicyTree) -> RemoteDebugging {
    let mut found = Vec::new();
    for (browser, dir) in LINUX_DIRS {
        let place = Place {
            browser,
            scope: Scope::Machine,
        };
        let files = match tree.json_files(Path::new(dir)) {
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
            let said = match tree.read(&file) {
                Ok(None) => Found::Absent,
                Ok(Some(bytes)) => json_says(&bytes),
                Err(error) => Found::Unreadable(format!("a file was not read ({})", error.kind())),
            };
            found.push((place, said));
        }
    }
    RemoteDebugging { found }
}

/// What a JSON policy file's `RemoteDebuggingAllowed` says.
pub fn json_says(bytes: &[u8]) -> Found {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_slice(bytes) else {
        return Found::Unreadable("a file is not a JSON object".to_owned());
    };
    match map.get(VALUE) {
        None => Found::Absent,
        Some(serde_json::Value::Bool(false)) => Found::Disallowed,
        Some(serde_json::Value::Bool(true)) => Found::Allowed,
        Some(_) => Found::Unreadable("the value is not a boolean".to_owned()),
    }
}
