//! The runtime's version against the minimum: what the check decides, without Windows.

use std::cmp::Ordering;

/// The oldest runtime the app starts on: `bundle.windows.minimumWebview2Version` in
/// `tauri.conf.json`, which the installer also uses (a test holds the two together). Raised only
/// on purpose (`docs/decisions/0035-release-builds.md`).
pub(super) const MINIMUM: &str = "155.0.0.0";

/// A `WebView2` version: major, minor, build and patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Version([u32; 4]);

impl Version {
    /// Reads `major.minor.build.patch`, as the runtime reports it ("155.0.3485.54"), and as
    /// `minimumWebview2Version` is written. A channel name after a space ("156.0.1.2 beta") is
    /// left out; fewer than four parts count the missing ones as 0. `None` for anything else.
    pub(super) fn parse(text: &str) -> Option<Version> {
        let number = text.split_whitespace().next()?;
        let mut parts = [0_u32; 4];
        for (index, part) in number.split('.').enumerate() {
            let slot = parts.get_mut(index)?;
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            *slot = part.parse().ok()?;
        }
        Some(Version(parts))
    }
}

/// What the check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdict {
    /// The runtime is there and new enough, or reports a version we do not read: start.
    Start,
    /// No runtime: show the dialog.
    Missing,
    /// A runtime older than the minimum: show the dialog.
    TooOld,
}

/// Decides from the version the runtime reported (`None` or empty when there is none) and the
/// minimum.
pub(super) fn verdict(installed: Option<&str>, minimum: Version) -> Verdict {
    let Some(installed) = installed.filter(|text| !text.trim().is_empty()) else {
        return Verdict::Missing;
    };
    match Version::parse(installed).map(|version| version.cmp(&minimum)) {
        Some(Ordering::Less) => Verdict::TooOld,
        Some(Ordering::Equal | Ordering::Greater) | None => Verdict::Start,
    }
}
