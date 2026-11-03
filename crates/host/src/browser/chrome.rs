//! Which Chrome a session's browser runs (S27, "Which Chrome"; decision record 0029). The host
//! looks on the launch-prep thread, never on the actor, and fills `{browser_executable}` with
//! what it found. None found is `BrowserNotInstalled`.
//!
//! In this order:
//! 1. Google Chrome installed on the machine: on macOS
//!    `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`, then the same under
//!    `~/Applications`; on Windows `%ProgramFiles%`, `%ProgramFiles(x86)%`, then `%LOCALAPPDATA%`,
//!    each with `\Google\Chrome\Application\chrome.exe`.
//! 2. On Windows only, Microsoft Edge (`%ProgramFiles(x86)%`, then `%ProgramFiles%`, with
//!    `\Microsoft\Edge\Application\msedge.exe`), only while [`EDGE_ALLOWED`] is true.
//! 3. Chrome for Testing that Clauseo downloaded into `<app data>/browsers/chrome-<version>/`,
//!    the highest version first, with the binary where the Chrome for Testing zip puts it.
//!    Skipped on Windows ARM64, where only a slow emulated x64 build exists.
//! 4. Otherwise none.
//!
//! On Linux (development and CI only) the host takes `/usr/bin/google-chrome`, then
//! `/usr/bin/chromium`.
//!
//! Every root comes in through [`ChromeRoots`], so the tests use fixture folders.

use std::path::{Path, PathBuf};

/// Whether Microsoft Edge may stand in for Chrome on Windows. False until G17 item 4 shows it
/// works with agent-browser (its issues #177 and #1867; agent-browser does not detect Edge).
pub const EDGE_ALLOWED: bool = false;

/// The folder under the app data folder that holds Chrome for Testing downloads.
pub const BROWSERS_DIR: &str = "browsers";

/// The prefix of one Chrome for Testing download's folder: `chrome-<version>`.
pub const CFT_PREFIX: &str = "chrome-";

/// The machine the host looks on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS on Apple silicon.
    MacArm64,
    /// macOS on Intel.
    MacX64,
    /// Windows on x64.
    WindowsX64,
    /// Windows on ARM64: no Chrome for Testing.
    WindowsArm64,
    /// Linux, for development and CI.
    Linux,
    /// Any other system: no browser.
    Other,
}

impl Platform {
    /// The machine this host was built for.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                Self::MacArm64
            } else {
                Self::MacX64
            }
        } else if cfg!(windows) {
            if cfg!(target_arch = "aarch64") {
                Self::WindowsArm64
            } else {
                Self::WindowsX64
            }
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }

    /// The Chrome for Testing binary inside one download's folder, as the zip lays it out, or
    /// `None` where Clauseo uses no Chrome for Testing.
    fn cft_binary(self) -> Option<PathBuf> {
        let mac = |folder: &str| {
            Path::new(folder)
                .join("Google Chrome for Testing.app")
                .join("Contents")
                .join("MacOS")
                .join("Google Chrome for Testing")
        };
        match self {
            Self::MacArm64 => Some(mac("chrome-mac-arm64")),
            Self::MacX64 => Some(mac("chrome-mac-x64")),
            Self::WindowsX64 => Some(Path::new("chrome-win64").join("chrome.exe")),
            Self::WindowsArm64 | Self::Linux | Self::Other => None,
        }
    }
}

/// Where the host looks. Production takes them from the system ([`ChromeRoots::system`]);
/// tests point each at a fixture folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeRoots {
    /// The machine.
    pub platform: Platform,
    /// The file system root for absolute paths: `/` on macOS and Linux. On macOS
    /// `Applications/...` is looked for under it, on Linux `usr/bin/...`.
    pub root: PathBuf,
    /// macOS: the home folder, for `~/Applications`.
    pub home: Option<PathBuf>,
    /// Windows: `%ProgramFiles%`.
    pub program_files: Option<PathBuf>,
    /// Windows: `%ProgramFiles(x86)%`.
    pub program_files_x86: Option<PathBuf>,
    /// Windows: `%LOCALAPPDATA%`.
    pub local_app_data: Option<PathBuf>,
    /// Clauseo's app data folder, which holds `browsers/`.
    pub app_data: PathBuf,
}

impl ChromeRoots {
    /// This machine's roots, with `app_data` the host's data folder. Reads the environment
    /// (`HOME`, `ProgramFiles`, `ProgramFiles(x86)`, `LOCALAPPDATA`), so it runs off the actor
    /// with the rest of the launch prep.
    pub fn system(app_data: &Path) -> Self {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Self {
            platform: Platform::current(),
            root: PathBuf::from("/"),
            home: var("HOME"),
            program_files: var("ProgramFiles"),
            program_files_x86: var("ProgramFiles(x86)"),
            local_app_data: var("LOCALAPPDATA"),
            app_data: app_data.to_owned(),
        }
    }

    /// The places to look, in order, before Chrome for Testing; Edge's only when
    /// `edge_allowed`.
    fn installed(&self, edge_allowed: bool) -> Vec<PathBuf> {
        let chrome_app = Path::new("Applications")
            .join("Google Chrome.app")
            .join("Contents")
            .join("MacOS")
            .join("Google Chrome");
        let chrome_exe = Path::new("Google")
            .join("Chrome")
            .join("Application")
            .join("chrome.exe");
        let edge_exe = Path::new("Microsoft")
            .join("Edge")
            .join("Application")
            .join("msedge.exe");
        match self.platform {
            Platform::MacArm64 | Platform::MacX64 => [Some(&self.root), self.home.as_ref()]
                .into_iter()
                .flatten()
                .map(|base| base.join(&chrome_app))
                .collect(),
            Platform::WindowsX64 | Platform::WindowsArm64 => {
                let chrome = [
                    &self.program_files,
                    &self.program_files_x86,
                    &self.local_app_data,
                ]
                .into_iter()
                .flatten()
                .map(|base| base.join(&chrome_exe));
                let edge = [&self.program_files_x86, &self.program_files]
                    .into_iter()
                    .flatten()
                    .filter(|_| edge_allowed)
                    .map(|base| base.join(&edge_exe));
                chrome.chain(edge).collect()
            }
            Platform::Linux => ["google-chrome", "chromium"]
                .into_iter()
                .map(|name| self.root.join("usr").join("bin").join(name))
                .collect(),
            Platform::Other => Vec::new(),
        }
    }
}

/// The Chrome this machine offers, or `None` (`BrowserNotInstalled`). File system calls: off
/// the actor.
pub fn find(roots: &ChromeRoots) -> Option<PathBuf> {
    find_with(roots, EDGE_ALLOWED)
}

/// [`find`] with the Edge switch given, so its test covers both values.
fn find_with(roots: &ChromeRoots, edge_allowed: bool) -> Option<PathBuf> {
    roots
        .installed(edge_allowed)
        .into_iter()
        .find(|path| is_file(path))
        .or_else(|| chrome_for_testing(roots))
}

/// The highest-versioned Chrome for Testing under `<app data>/browsers/` whose binary is there.
/// A version folder must be a real folder (a link is passed over, by `file_type`, which does not
/// follow it), and the binary, links resolved, must still sit under the browsers folder, so a
/// link planted in the folder cannot pick another program. The folder must be outside what an
/// agent can write (record 0029).
fn chrome_for_testing(roots: &ChromeRoots) -> Option<PathBuf> {
    let binary = roots.platform.cft_binary()?;
    let browsers = roots.app_data.join(BROWSERS_DIR);
    let real_browsers = dunce::canonicalize(&browsers).ok()?;
    let mut found: Vec<(Vec<u64>, PathBuf)> = std::fs::read_dir(&browsers)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            if !entry.file_type().ok()?.is_dir() {
                return None;
            }
            let name = entry.file_name();
            let version = version(name.to_str()?.strip_prefix(CFT_PREFIX)?)?;
            let path = entry.path().join(&binary);
            let inside =
                dunce::canonicalize(&path).is_ok_and(|real| real.starts_with(&real_browsers));
            (inside && is_file(&path)).then_some((version, path))
        })
        .collect();
    found.sort();
    found.pop().map(|(_, path)| path)
}

/// A dotted version, such as `131.0.6778.85`, as numbers to compare; `None` when it is not one.
fn version(text: &str) -> Option<Vec<u64>> {
    text.split('.')
        .map(|part| {
            (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        })
        .collect()
}

/// A regular file, following links (an installed Chrome may be a link).
fn is_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

#[cfg(test)]
mod tests;
