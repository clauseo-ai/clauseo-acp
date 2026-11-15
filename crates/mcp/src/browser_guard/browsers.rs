//! Windows: which browsers the guard tries, in order, and why none may be left (route 2,
//! decision record 0060).
//!
//! The browser runs over a `DevTools` pipe (`chrome/pipe.rs`), which only a browser installed
//! under Program Files can serve: the spike's Chrome for Testing, unpacked in a per-user folder
//! whose ACL lacks ALL APPLICATION PACKAGES, failed at its own sandbox ("cannot access
//! executable"). A per-user Chrome install (`%LOCALAPPDATA%\Google\Chrome\Application`) sits
//! under the same folder rules, so it is taken to fail the same way (inferred, not tried). So,
//! in order:
//!
//! 1. the browser the host chose (`AGENT_BROWSER_EXECUTABLE_PATH`, the same order below on
//!    Windows: `crates/host/src/browser/chrome.rs`), only when it lies under `%ProgramFiles%` or
//!    `%ProgramFiles(x86)%` (Edge installs itself under the latter); any other path is set aside
//!    ([`Choice::refused`]) and the guard's own search goes on;
//! 2. Microsoft Edge, `<Program Files (x86)>\Microsoft\Edge\Application\msedge.exe`, then the
//!    same under `<Program Files>`: on every Windows 11, signed and updated by Microsoft;
//! 3. Google Chrome, `<Program Files>\Google\Chrome\Application\chrome.exe`, then under
//!    `<Program Files (x86)>`: where Edge was removed (the EEA allows it) or its debugging is
//!    blocked.
//!
//! Each path appears once (compared without case) and only if it is a file. A browser whose
//! firm policy sets `RemoteDebuggingAllowed` to 0, or `DeveloperToolsAvailability` to 2
//! (`SOFTWARE\Policies\Microsoft\Edge` or `SOFTWARE\Policies\Google\Chrome`; for each value the
//! machine's over the user's), would not serve its pipe; it is set aside up front rather than
//! waited for. A policy set any other way (MDM or cloud management, not the registry) is not
//! seen here: that browser is tried, fails its start wait (15 s), and the next is tried. Chrome
//! for Testing and a per-user Chrome are never tried on Windows.
//!
//! When the list is empty the reason is one of three, each with words for the agent
//! ([`Unavailable`]); a browser that is tried and fails is the plain "could not start".

use std::path::{Component, Path, PathBuf};

/// The policy value both browsers read (`remote_debugging_server.cc`,
/// `prefs::kDevToolsRemoteDebuggingAllowed`): 0 turns remote debugging off.
pub(crate) const POLICY_VALUE: &str = "RemoteDebuggingAllowed";

/// The other policy value that blocks: `DeveloperToolsAvailability` 2 disallows the developer
/// tools, and the browser then refuses to let a client attach to its pages.
pub(crate) const DEVTOOLS_VALUE: &str = "DeveloperToolsAvailability";

/// [`DEVTOOLS_VALUE`]'s value that blocks: developer tools disallowed.
pub(crate) const DEVTOOLS_DISALLOWED: u32 = 2;

/// Tests only (the `test-support` feature, never in a shipped build): the folder the guard
/// reads as both Program Files folders.
#[cfg(all(windows, feature = "test-support"))]
pub(crate) const PROGRAM_FILES_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_PROGRAM_FILES";

/// Who makes a browser, which says where its policy is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Vendor {
    Edge,
    Chrome,
}

impl Vendor {
    /// The policy key under `HKLM` and `HKCU`.
    pub(crate) fn policy_key(self) -> &'static str {
        match self {
            Vendor::Edge => r"SOFTWARE\Policies\Microsoft\Edge",
            Vendor::Chrome => r"SOFTWARE\Policies\Google\Chrome",
        }
    }

    /// The vendor of the browser at `path`, by its file name; none for any other.
    pub(crate) fn of(path: &Path) -> Option<Vendor> {
        // The last component by either slash, so a Windows path reads the same on any system.
        let text = path.to_str()?;
        let name = text.rsplit(['/', '\\']).next()?;
        if name.eq_ignore_ascii_case("msedge.exe") {
            Some(Vendor::Edge)
        } else if name.eq_ignore_ascii_case("chrome.exe") {
            Some(Vendor::Chrome)
        } else {
            None
        }
    }
}

/// The folders the guard looks in, from its environment: `ProgramFiles(x86)`, `ProgramW6432`
/// or else `ProgramFiles`, and `LOCALAPPDATA` (only to tell a per-user Chrome apart). Any may
/// be missing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Folders {
    pub(crate) x86: Option<PathBuf>,
    pub(crate) native: Option<PathBuf>,
    pub(crate) local_app_data: Option<PathBuf>,
    /// Tests only (the `test-support` knob): the host's choice is tried wherever it lies, since
    /// a test's stand-in browser is never under its Program Files folder.
    pub(crate) any_host_choice: bool,
}

/// The browsers to try, in order, and those set aside.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Choice {
    pub(crate) try_in_order: Vec<PathBuf>,
    /// Installed under Program Files, but blocked by their firm policy.
    pub(crate) blocked: Vec<PathBuf>,
    /// The host's choice, when it lies outside Program Files: never tried.
    pub(crate) refused: Option<PathBuf>,
    /// A Chrome installed for this user only (`%LOCALAPPDATA%`): never tried.
    pub(crate) per_user: Option<PathBuf>,
}

/// Why no browser can be tried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unavailable {
    /// Neither Edge nor Chrome is installed under Program Files, nor a per-user Chrome.
    NotInstalled,
    /// None under Program Files, but a Chrome installed for this user only, which the guard
    /// cannot use.
    PerUserOnly,
    /// Each one installed under Program Files is blocked by its firm policy: which ones.
    Blocked { edge: bool, chrome: bool },
}

impl Unavailable {
    /// The guard's stderr line.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Unavailable::NotInstalled => {
                "no browser to start: neither Microsoft Edge nor Google Chrome is installed under \
                 Program Files"
            }
            Unavailable::PerUserOnly => {
                "no browser to start: the only Google Chrome is a per-user install, which cannot \
                 serve a DevTools pipe"
            }
            Unavailable::Blocked { .. } => {
                "no browser to start: the firm's policy (RemoteDebuggingAllowed=0 or \
                 DeveloperToolsAvailability=2) blocks every installed Microsoft Edge and Google \
                 Chrome"
            }
        }
    }

    /// What the agent's tool call is told, after the plain "could not start". The same advice
    /// as the lawyer's panel (UC-550 to UC-553).
    pub(crate) fn agent_note(self) -> &'static str {
        match self {
            Unavailable::NotInstalled => {
                "Neither Microsoft Edge nor Google Chrome is installed on this computer. Tell the \
                 lawyer that installing Microsoft Edge or Google Chrome and starting a new \
                 conversation would give you a browser."
            }
            Unavailable::PerUserOnly => {
                "The only Google Chrome on this computer is installed just for the lawyer, and \
                 Clauseo can't use that copy. Tell the lawyer to install Microsoft Edge, or to \
                 ask the firm's IT to install Google Chrome for everyone on this computer, then \
                 start a new conversation."
            }
            Unavailable::Blocked {
                edge: true,
                chrome: false,
            } => {
                "The firm's settings on this computer don't let apps control Microsoft Edge. \
                 Tell the lawyer to ask the firm's IT to allow this for Clauseo, then start a new \
                 conversation."
            }
            Unavailable::Blocked {
                edge: false,
                chrome: true,
            } => {
                "The firm's settings on this computer don't let apps control Google Chrome. Tell \
                 the lawyer to ask the firm's IT to allow this for Clauseo, then start a new \
                 conversation."
            }
            Unavailable::Blocked { .. } => {
                "The firm's settings on this computer don't let apps control Microsoft Edge or \
                 Google Chrome. Tell the lawyer to ask the firm's IT to allow this for Clauseo, \
                 then start a new conversation."
            }
        }
    }
}

impl Choice {
    /// Why nothing can be tried; `None` while something can. A firm's block comes first: a
    /// per-user Chrome beside a blocked Edge is told as the block.
    pub(crate) fn unavailable(&self) -> Option<Unavailable> {
        if !self.try_in_order.is_empty() {
            return None;
        }
        if !self.blocked.is_empty() {
            let has = |vendor| {
                self.blocked
                    .iter()
                    .any(|path| Vendor::of(path) == Some(vendor))
            };
            return Some(Unavailable::Blocked {
                edge: has(Vendor::Edge),
                chrome: has(Vendor::Chrome),
            });
        }
        if self.per_user.is_some() {
            return Some(Unavailable::PerUserOnly);
        }
        Some(Unavailable::NotInstalled)
    }

    /// Moves `first` to the front, if it is in the list: the browser that started last time.
    pub(crate) fn prefer(&mut self, first: &Path) {
        if let Some(at) = self.try_in_order.iter().position(|path| same(path, first)) {
            let path = self.try_in_order.remove(at);
            self.try_in_order.insert(0, path);
        }
    }
}

/// The order above, for the host's choice `host`. Pure: `is_file` and `blocked` are the
/// caller's.
pub(crate) fn choose(
    host: &Path,
    dirs: &Folders,
    is_file: impl Fn(&Path) -> bool,
    blocked: impl Fn(Vendor) -> bool,
) -> Choice {
    let edge = Path::new(r"Microsoft\Edge\Application\msedge.exe");
    let chrome = Path::new(r"Google\Chrome\Application\chrome.exe");
    let fixed = [
        dirs.x86.as_ref().map(|dir| dir.join(edge)),
        dirs.native.as_ref().map(|dir| dir.join(edge)),
        dirs.native.as_ref().map(|dir| dir.join(chrome)),
        dirs.x86.as_ref().map(|dir| dir.join(chrome)),
    ];
    let mut choice = Choice::default();
    let roots = [dirs.x86.as_deref(), dirs.native.as_deref()];
    let host = if dirs.any_host_choice || roots.into_iter().flatten().any(|root| under(host, root))
    {
        Some(host.to_owned())
    } else {
        choice.refused = Some(host.to_owned());
        None
    };
    for path in host.into_iter().chain(fixed.into_iter().flatten()) {
        let seen = choice
            .try_in_order
            .iter()
            .chain(&choice.blocked)
            .any(|known| same(known, &path));
        if seen || !is_file(&path) {
            continue;
        }
        if Vendor::of(&path).is_some_and(&blocked) {
            choice.blocked.push(path);
        } else {
            choice.try_in_order.push(path);
        }
    }
    choice.per_user = dirs
        .local_app_data
        .as_ref()
        .map(|dir| dir.join(chrome))
        .filter(|path| is_file(path));
    choice
}

/// The same path, compared without ASCII case and with either slash.
fn same(a: &Path, b: &Path) -> bool {
    fold(a) == fold(b)
}

/// A path as Windows compares it: backslashes, no ASCII case.
fn fold(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

/// Whether `path` lies inside the folder `root` (without case, either slash), with no `..` in
/// it to climb back out.
fn under(path: &Path, root: &Path) -> bool {
    let text = fold(path);
    let climbs = path.components().any(|part| part == Component::ParentDir)
        || text.split('\\').any(|part| part == "..");
    let mut root = fold(root);
    if !root.ends_with('\\') {
        root.push('\\');
    }
    !climbs && text.len() > root.len() && text.starts_with(&root)
}

/// The policy's verdict from the machine's value and then the user's, each value on its own:
/// blocked when the first `RemoteDebuggingAllowed` set is 0, or the first
/// `DeveloperToolsAvailability` set is 2. A value that is missing, or could not be read, does
/// not block (a browser that then refuses its pipe fails its start like any other).
pub(crate) fn blocked_by(remote_debugging: [Option<u32>; 2], devtools: [Option<u32>; 2]) -> bool {
    let [machine, user] = remote_debugging;
    let [dev_machine, dev_user] = devtools;
    machine.or(user) == Some(0) || dev_machine.or(dev_user) == Some(DEVTOOLS_DISALLOWED)
}

/// This machine's choice, for the host's `host`.
#[cfg(windows)]
pub(crate) fn system(host: &Path) -> Choice {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    // Tests only: a folder of their own instead of Program Files, so a CI machine's real Edge
    // and Chrome are never started, and the test's stand-in as the host's choice.
    #[cfg(feature = "test-support")]
    if let Some(dir) = var(PROGRAM_FILES_FOR_TESTS) {
        let dirs = Folders {
            x86: Some(dir.clone()),
            native: Some(dir),
            local_app_data: None,
            any_host_choice: true,
        };
        return choose(host, &dirs, Path::is_file, system_blocks);
    }
    let dirs = Folders {
        x86: var("ProgramFiles(x86)"),
        native: var("ProgramW6432").or_else(|| var("ProgramFiles")),
        local_app_data: var("LOCALAPPDATA"),
        any_host_choice: false,
    };
    choose(host, &dirs, Path::is_file, system_blocks)
}

/// Whether this machine's registry policy blocks `vendor` ([`blocked_by`]).
#[cfg(windows)]
fn system_blocks(vendor: Vendor) -> bool {
    let key = vendor.policy_key();
    let read = |value| {
        [
            policy::dword(policy::Hive::Machine, key, value),
            policy::dword(policy::Hive::User, key, value),
        ]
    };
    blocked_by(read(POLICY_VALUE), read(DEVTOOLS_VALUE))
}

#[cfg(windows)]
mod policy;

#[cfg(test)]
mod tests;
