//! The version list and the URL checks: which zip the download takes, decided before any
//! download starts. Pure: no file, network or process call.
//!
//! The list (`last-known-good-versions-with-downloads.json`) names, per channel, a `version` and
//! per platform a `url`. The host takes Stable's `chrome` entry for this platform. The version
//! must be dotted digits, and the zip's URL must be exactly what Google's layout gives that
//! version and platform: `https`, host `storage.googleapis.com`, path
//! `/chrome-for-testing-public/<version>/<platform>/chrome-<platform>.zip`, with no port, user,
//! query or fragment. Anything else is `host_refused`. The list gives no hash (checked
//! 2026-11-02), so the integrity check is HTTPS to Google's host and the size match (0029).

use clauseo_protocol::BrowserInstallFailure;
use serde_json::Value;

use super::Failure;
use crate::browser::chrome;

/// The version list's scheme and host.
const VERSIONS_ORIGIN: &str = "https://googlechromelabs.github.io/";

/// The zips' scheme, host and first path segment.
const ZIP_ORIGIN: &str = "https://storage.googleapis.com/chrome-for-testing-public/";

/// The longest version the host takes: four numbers of a few digits each fit well inside.
const MAX_VERSION_BYTES: usize = 32;

/// The zip the version list names for this platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// Stable's version, dotted digits.
    pub version: String,
    /// The zip's URL, checked.
    pub url: String,
}

/// The version list's own URL must be `https` on `googlechromelabs.github.io`.
pub fn check_versions_url(url: &str) -> Result<(), Failure> {
    let path = url.strip_prefix(VERSIONS_ORIGIN).ok_or_else(|| {
        refused(format!(
            "the version list's URL {url} is not https on googlechromelabs.github.io"
        ))
    })?;
    if path.is_empty() || path.contains(['?', '#', '@', '\\']) {
        return Err(refused(format!(
            "the version list's URL {url} has a bad path"
        )));
    }
    Ok(())
}

/// Stable's `chrome` download for `platform` (`mac-arm64`, `mac-x64` or `win64`) from the version
/// list `body`, its version and URL checked.
pub fn stable_download(body: &[u8], platform: &str) -> Result<Download, Failure> {
    let list: Value = serde_json::from_slice(body)
        .map_err(|error| refused(format!("the version list is not JSON: {error}")))?;
    let stable = list
        .get("channels")
        .and_then(|channels| channels.get("Stable"))
        .ok_or_else(|| refused("the version list has no Stable channel"))?;
    let version = stable
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| refused("Stable has no version"))?;
    check_version(version)?;
    let url = stable
        .get("downloads")
        .and_then(|downloads| downloads.get("chrome"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|entry| entry.get("platform").and_then(Value::as_str) == Some(platform))
        .and_then(|entry| entry.get("url"))
        .and_then(Value::as_str)
        .ok_or_else(|| refused(format!("Stable has no chrome download for {platform}")))?;
    check_zip_url(url, version, platform)?;
    Ok(Download {
        version: version.to_owned(),
        url: url.to_owned(),
    })
}

/// A version must be dotted digits (`131.0.6778.85`), as `chrome::find` reads folder names.
pub fn check_version(version: &str) -> Result<(), Failure> {
    if version.len() <= MAX_VERSION_BYTES && chrome::version(version).is_some() {
        Ok(())
    } else {
        Err(refused(format!(
            "the version {version:?} is not dotted digits"
        )))
    }
}

/// The zip's URL must be exactly Google's for `version` and `platform`; the message says which
/// part differs.
pub fn check_zip_url(url: &str, version: &str, platform: &str) -> Result<(), Failure> {
    let expected = format!("{ZIP_ORIGIN}{version}/{platform}/chrome-{platform}.zip");
    if url == expected {
        return Ok(());
    }
    let why = if !url.starts_with("https://") {
        "it is not https"
    } else if !url.starts_with("https://storage.googleapis.com/") {
        "its host is not storage.googleapis.com"
    } else if !url.starts_with(&format!("{ZIP_ORIGIN}{version}/")) {
        "its version is not Stable's"
    } else {
        "its path is not Google's for this platform"
    };
    Err(refused(format!("the zip's URL {url} is refused: {why}")))
}

/// A `host_refused` failure.
fn refused(message: impl Into<String>) -> Failure {
    Failure::new(BrowserInstallFailure::HostRefused, message)
}
