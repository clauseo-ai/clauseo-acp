//! Which zip the download takes, decided before any download starts, from the signed browser
//! pins (`browser::pins`). Pure: no file, network or process call.
//!
//! The URL is never read from anywhere: it is built from the fixed template `ZIP_ORIGIN` +
//! `{version}/{platform}/{name}-{platform}.zip` ([`zip_url`]), that is
//! `https://storage.googleapis.com/chrome-for-testing-public/{version}/...`, with `version` the
//! pinned one (dotted digits of bounded length, [`check_version`], checked when the pins were
//! read and again here), `platform` Chrome for Testing's name for this machine
//! (`Platform::cft_name`) and `name` a fixed download name (`pins::DOWNLOAD_CHROME`): both
//! `&'static str` from the code, never a key of the manifest. The pin gives the zip's sha256 and
//! size, which the download is checked against.

use clauseo_protocol::BrowserInstallFailure;

use super::Failure;
use crate::browser::chrome;
use crate::browser::pins::{DOWNLOAD_CHROME, Pins};

/// The zips' scheme, host and first path segment.
const ZIP_ORIGIN: &str = "https://storage.googleapis.com/chrome-for-testing-public/";

/// The longest version the host takes: four numbers of a few digits each fit well inside.
const MAX_VERSION_BYTES: usize = 32;

/// The zip the pins name for this platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// The pinned version, dotted digits.
    pub version: String,
    /// The zip's URL, from the template.
    pub url: String,
    /// The zip's sha256, 64 lowercase hex digits.
    pub sha256: String,
    /// The zip's size in bytes.
    pub size: u64,
}

/// The `chrome` download for `platform` (`mac-arm64`, `mac-x64` or `win64`, from
/// `Platform::cft_name`) from `pins`: `list_unavailable` without pins, `unsupported` when they
/// pin no `chrome` zip for `platform`, `host_refused` for a version that is not dotted digits.
pub fn pinned_download(pins: Option<&Pins>, platform: &'static str) -> Result<Download, Failure> {
    let pins = pins.ok_or_else(|| {
        Failure::new(
            BrowserInstallFailure::ListUnavailable,
            "no signed browser list verifies",
        )
    })?;
    let version = pins.version();
    check_version(version)?;
    let pin = pins.pin(DOWNLOAD_CHROME, platform).ok_or_else(|| {
        Failure::new(
            BrowserInstallFailure::Unsupported,
            format!("the browser list pins no {DOWNLOAD_CHROME} zip for {platform}"),
        )
    })?;
    Ok(Download {
        version: version.to_owned(),
        url: zip_url(version, platform, DOWNLOAD_CHROME),
        sha256: pin.sha256.clone(),
        size: pin.size,
    })
}

/// The zip's URL from the fixed template, for a checked `version`, this machine's `platform`
/// and the download `name` (`chrome`; `chrome-headless-shell` later), both fixed in the code.
pub fn zip_url(version: &str, platform: &'static str, name: &'static str) -> String {
    format!("{ZIP_ORIGIN}{version}/{platform}/{name}-{platform}.zip")
}

/// A version must be dotted digits (`131.0.6778.85`) of at most 32 bytes, as `chrome::find`
/// reads folder names.
pub fn check_version(version: &str) -> Result<(), Failure> {
    if version.len() <= MAX_VERSION_BYTES && chrome::version(version).is_some() {
        Ok(())
    } else {
        Err(Failure::new(
            BrowserInstallFailure::HostRefused,
            format!("the version {version:?} is not dotted digits"),
        ))
    }
}
