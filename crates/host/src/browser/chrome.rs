//! Which Chrome a session's browser runs (S27, "Which Chrome"; decision record 0029). The host
//! looks on the launch-prep thread, never on the actor, and fills `{browser_executable}` with
//! what it found. Before it looks, the launch prep asks the firm's Chrome and Edge policy
//! (`super::rd_policy`), so nothing here runs when that blocks.
//!
//! On macOS and Windows x64 the guarded browser is only ever the Chrome for Testing that Clauseo
//! downloaded (`super::install`), at exactly the version the newest verified browser pins
//! (`super::pins`) name: `<app data>/browsers/chrome-<pinned version>/`, with the binary where
//! the zip puts it. A Google Chrome or Microsoft Edge installed on the machine is never used: the
//! firm updates those, not Clauseo, so the pins could not vouch for them. Windows ARM64, where
//! only a slow emulated x64 build exists, gets none. [`find_checked`] judges, in order:
//! 1. No `browsers/chrome-<version>/` folder at all: `BrowserNotInstalled` (the app offers the
//!    download). The pins are read only past this point.
//! 2. No pins that verify, or the newest dated over a day ahead of the clock or below the floor
//!    (`PinsSource::newest`): `BrowserListUnavailable`. Pins over 45 days old:
//!    `BrowserListExpired`.
//! 3. No folder named for the pinned version, compared as the whole version string (a folder
//!    above the pin as much as one below): `BrowserOutOfDate` (the app offers the download,
//!    which fetches the pinned version).
//! 4. The pinned folder against its install record (`install::record`), walked and hashed
//!    again at every start, before the path goes to the guard: a missing record, or a file
//!    added, removed or changed, is `BrowserNotInstalled` with a warning, so the download, which
//!    replaces the folder and its record, is offered again.
//!
//! On Linux, which Clauseo does not ship (development and CI only), the host takes
//! `/usr/bin/google-chrome`, then `/usr/bin/chromium`, unpinned and unchecked. That code is built
//! on Linux only, so nothing on macOS or Windows can reach it.
//!
//! Every root comes in through [`ChromeRoots`], so the tests use fixture folders.

use std::path::{Path, PathBuf};

use clauseo_blocklist::Generated;
use clauseo_protocol::{Timestamp, ToolsUnavailableReason};

use super::install::record;
use super::pins::Pins;

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

    /// Chrome for Testing's name for this platform in its version list and its zip
    /// (`chrome-<name>.zip`), or `None` where Clauseo downloads no Chrome for Testing.
    pub fn cft_name(self) -> Option<&'static str> {
        match self {
            Self::MacArm64 => Some("mac-arm64"),
            Self::MacX64 => Some("mac-x64"),
            Self::WindowsX64 => Some("win64"),
            Self::WindowsArm64 | Self::Linux | Self::Other => None,
        }
    }

    /// The Chrome for Testing binary inside one download's folder, as the zip lays it out, or
    /// `None` where Clauseo uses no Chrome for Testing.
    pub(crate) fn cft_binary(self) -> Option<PathBuf> {
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
    /// Linux only (development and CI): the file system root `usr/bin/...` is looked for under,
    /// `/` in production. Never read on another platform.
    pub root: PathBuf,
    /// Clauseo's app data folder, which holds `browsers/` and `browser-pins/`.
    pub app_data: PathBuf,
}

impl ChromeRoots {
    /// This machine's roots, with `app_data` the host's data folder.
    pub fn system(app_data: &Path) -> Self {
        Self {
            platform: Platform::current(),
            root: PathBuf::from("/"),
            app_data: app_data.to_owned(),
        }
    }
}

/// The Chrome the launch prep picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    /// Its binary.
    pub path: PathBuf,
    /// The `published` time of the pins it was judged by, for the freshness notice
    /// (`pins::notice`). `None` for Linux's unpinned development Chrome.
    pub pins_published: Option<Generated>,
}

/// The Chrome for this session, judged as the module says: the pinned Chrome for Testing whose
/// folder matches its install record, with the date of the pins (`pins()`, called only once a
/// downloaded folder is there) that named it. Else why not: `BrowserNotInstalled`,
/// `BrowserListUnavailable`, `BrowserListExpired` or `BrowserOutOfDate`. Reads and hashes the
/// whole folder: off the actor.
pub fn find_checked(
    roots: &ChromeRoots,
    now: Timestamp,
    pins: impl FnOnce() -> Option<Pins>,
) -> Result<Picked, ToolsUnavailableReason> {
    #[cfg(target_os = "linux")]
    if roots.platform == Platform::Linux {
        return linux_development(roots)
            .map(|path| Picked {
                path,
                pins_published: None,
            })
            .ok_or(ToolsUnavailableReason::BrowserNotInstalled);
    }
    let (Some(binary), Some(platform)) = (roots.platform.cft_binary(), roots.platform.cft_name())
    else {
        return Err(ToolsUnavailableReason::BrowserNotInstalled);
    };
    let browsers = roots.app_data.join(BROWSERS_DIR);
    let downloaded = downloaded_versions(&browsers);
    if downloaded.is_empty() {
        return Err(ToolsUnavailableReason::BrowserNotInstalled);
    }
    let pins = pins().ok_or(ToolsUnavailableReason::BrowserListUnavailable)?;
    if super::pins::expired(pins.published(), now) {
        tracing::info!(
            published = pins.published().as_str(),
            "the browser list is over 45 days old; the downloaded Chrome for Testing is refused"
        );
        return Err(ToolsUnavailableReason::BrowserListExpired);
    }
    let pinned = pins.version();
    if !downloaded.iter().any(|version| version == pinned) {
        tracing::info!(
            pinned,
            run = pins.run(),
            "no downloaded Chrome for Testing is the pinned version"
        );
        return Err(ToolsUnavailableReason::BrowserOutOfDate);
    }
    let folder = browsers.join(format!("{CFT_PREFIX}{pinned}"));
    if let Err(refusal) = record::verify(&roots.app_data, platform, pinned, &folder) {
        tracing::warn!(
            %refusal,
            version = pinned,
            "the downloaded Chrome for Testing does not match its install record; refused"
        );
        return Err(ToolsUnavailableReason::BrowserNotInstalled);
    }
    let path = folder.join(binary);
    // The record checked every link in the folder; the binary must still resolve inside it.
    let inside = dunce::canonicalize(&browsers).is_ok_and(|real_browsers| {
        dunce::canonicalize(&path).is_ok_and(|real| real.starts_with(&real_browsers))
    });
    if !(inside && is_file(&path)) {
        tracing::warn!("the downloaded Chrome for Testing has no binary; refused");
        return Err(ToolsUnavailableReason::BrowserNotInstalled);
    }
    Ok(Picked {
        path,
        pins_published: Some(pins.published()),
    })
}

/// The versions of the Chrome for Testing folders under `browsers`, as their names spell them:
/// real folders only (a link is passed over, by `file_type`, which does not follow it) named
/// `chrome-<dotted digits>`. The folder must be outside what an agent can write (record 0029).
fn downloaded_versions(browsers: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(browsers) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name();
            let found = name.to_str()?.strip_prefix(CFT_PREFIX)?;
            version(found).is_some().then(|| found.to_owned())
        })
        .collect()
}

/// Linux (development and CI only, never shipped): `/usr/bin/google-chrome`, then
/// `/usr/bin/chromium`, under `roots.root`; unpinned and unchecked.
#[cfg(target_os = "linux")]
fn linux_development(roots: &ChromeRoots) -> Option<PathBuf> {
    ["google-chrome", "chromium"]
        .into_iter()
        .map(|name| roots.root.join("usr").join("bin").join(name))
        .find(|path| is_file(path))
}

/// A dotted version, such as `131.0.6778.85`, as numbers to compare; `None` when it is not one.
pub(crate) fn version(text: &str) -> Option<Vec<u64>> {
    text.split('.')
        .map(|part| {
            (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        })
        .collect()
}

/// A regular file, following links (Linux's `/usr/bin/google-chrome` is one).
fn is_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

#[cfg(test)]
mod tests;
