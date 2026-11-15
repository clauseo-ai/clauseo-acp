//! Windows: which browsers the guard tries, in order, and why none may be left (route 2,
//! decision record 0060).
//!
//! The browser runs over a `DevTools` pipe (`chrome/pipe.rs`), which only a browser installed
//! under Program Files can serve: the spike's Chrome for Testing, unpacked in a per-user folder
//! whose ACL lacks ALL APPLICATION PACKAGES, failed at its own sandbox ("cannot access
//! executable"), and a per-user Chrome install has the same folder ACL. So, in order:
//!
//! 1. the browser the host chose (`AGENT_BROWSER_EXECUTABLE_PATH`, the same order below on
//!    Windows: `crates/host/src/browser/chrome.rs`);
//! 2. Microsoft Edge, `<Program Files (x86)>\Microsoft\Edge\Application\msedge.exe`, then the
//!    same under `<Program Files>`: on every Windows 11, signed and updated by Microsoft;
//! 3. Google Chrome, `<Program Files>\Google\Chrome\Application\chrome.exe`, then under
//!    `<Program Files (x86)>`: where Edge was removed (the EEA allows it) or its debugging is
//!    blocked.
//!
//! Each path appears once (compared without case) and only if it is a file. A browser whose
//! firm policy sets `RemoteDebuggingAllowed` to 0 (`SOFTWARE\Policies\Microsoft\Edge` or
//! `SOFTWARE\Policies\Google\Chrome`, the machine's value over the user's) would not open its
//! pipe; it is set aside up front rather than waited for. Chrome for Testing is never tried on
//! Windows.
//!
//! When the list is empty the reason is one of two, each with words for the agent
//! ([`Unavailable`]); a browser that is tried and fails is the plain "could not start".

use std::path::{Path, PathBuf};

/// The policy value both browsers read (`remote_debugging_server.cc`,
/// `prefs::kDevToolsRemoteDebuggingAllowed`).
pub(crate) const POLICY_VALUE: &str = "RemoteDebuggingAllowed";

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

/// The Program Files folders, from the guard's environment (`ProgramFiles(x86)`, and
/// `ProgramW6432` or else `ProgramFiles`). Either may be missing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProgramFiles {
    pub(crate) x86: Option<PathBuf>,
    pub(crate) native: Option<PathBuf>,
}

/// The browsers to try, in order, and those a policy set aside.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Choice {
    pub(crate) try_in_order: Vec<PathBuf>,
    pub(crate) blocked: Vec<PathBuf>,
}

/// Why no browser can be tried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unavailable {
    /// Neither Edge nor Chrome is installed under Program Files.
    NotInstalled,
    /// Each one installed is blocked by its firm policy.
    Blocked,
}

impl Unavailable {
    /// The guard's stderr line.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Unavailable::NotInstalled => {
                "no browser to start: neither Microsoft Edge nor Google Chrome is installed under \
                 Program Files"
            }
            Unavailable::Blocked => {
                "no browser to start: the firm's policy RemoteDebuggingAllowed=0 blocks every \
                 installed Microsoft Edge and Google Chrome"
            }
        }
    }

    /// What the agent's tool call is told, after the plain "could not start".
    pub(crate) fn agent_note(self) -> &'static str {
        match self {
            Unavailable::NotInstalled => {
                "Neither Microsoft Edge nor Google Chrome is installed on this computer. Tell the \
                 lawyer that installing Microsoft Edge or Google Chrome and starting a new \
                 conversation would give you a browser."
            }
            Unavailable::Blocked => {
                "The firm's settings on this computer stop apps from using Microsoft Edge and \
                 Google Chrome. Tell the lawyer to ask the firm's IT to allow it."
            }
        }
    }
}

impl Choice {
    /// Why nothing can be tried; `None` while something can.
    pub(crate) fn unavailable(&self) -> Option<Unavailable> {
        match (self.try_in_order.is_empty(), self.blocked.is_empty()) {
            (false, _) => None,
            (true, true) => Some(Unavailable::NotInstalled),
            (true, false) => Some(Unavailable::Blocked),
        }
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
    dirs: &ProgramFiles,
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
    for path in std::iter::once(host.to_owned()).chain(fixed.into_iter().flatten()) {
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
    choice
}

/// The same path, compared without ASCII case and with either slash.
fn same(a: &Path, b: &Path) -> bool {
    let fold = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase()
    };
    fold(a) == fold(b)
}

/// The policy's verdict from the machine's value and then the user's: blocked when the first
/// one set is 0. A value that is missing, or could not be read, does not block (a browser that
/// then refuses its pipe fails its start like any other).
pub(crate) fn blocked_by(machine: Option<u32>, user: Option<u32>) -> bool {
    machine.or(user) == Some(0)
}

/// This machine's choice, for the host's `host`.
#[cfg(windows)]
pub(crate) fn system(host: &Path) -> Choice {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    // Tests only: a folder of their own instead of Program Files, so a CI machine's real Edge
    // and Chrome are never started.
    #[cfg(feature = "test-support")]
    let for_tests = var(PROGRAM_FILES_FOR_TESTS).map(|dir| ProgramFiles {
        x86: Some(dir.clone()),
        native: Some(dir),
    });
    #[cfg(not(feature = "test-support"))]
    let for_tests = None;
    let dirs = for_tests.unwrap_or_else(|| ProgramFiles {
        x86: var("ProgramFiles(x86)"),
        native: var("ProgramW6432").or_else(|| var("ProgramFiles")),
    });
    choose(host, &dirs, Path::is_file, |vendor| {
        let key = vendor.policy_key();
        blocked_by(
            policy::dword(policy::Hive::Machine, key, POLICY_VALUE),
            policy::dword(policy::Hive::User, key, POLICY_VALUE),
        )
    })
}

#[cfg(windows)]
mod policy;

#[cfg(test)]
mod tests;
