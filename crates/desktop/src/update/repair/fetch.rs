//! The two downloads: the installer's `.sig` (small, read whole) and the installer itself,
//! streamed into the attempt's file. The client is the updater's ([`super::super::plugin::client`]:
//! HTTPS only, redirects included, and its timeouts), so a repair reaches the release the way an
//! update does.

use std::time::Instant;

use url::Url;

/// The largest `.sig` read: Tauri's is base64 of a few hundred bytes of minisign text.
pub(super) const SIGNATURE_MAX: usize = 16 * 1024;

/// The largest installer written: about three times today's per-user installer (test-v0.0.3's
/// `Clauseo_0.0.3_x64-setup.exe` is 21,283,414 bytes, the arm64 one 19,797,588), so a hostile or
/// broken answer can neither fill the lawyer's disk nor, since the check reads the whole file
/// into memory (#728's `check_update_file` takes the bytes), take much memory. Raise it with the
/// installer.
pub(super) const INSTALLER_MAX: u64 = 64 * 1024 * 1024;

/// Why a download failed, as a fixed line for host.log: never the URL or reqwest's own text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Failed(pub(super) &'static str);

/// The `.sig` at `url`: its text, trimmed, at most [`SIGNATURE_MAX`] bytes.
pub(super) async fn signature(
    client: &reqwest::Client,
    url: &Url,
    deadline: Instant,
) -> Result<String, Failed> {
    let mut response = get(client, url, deadline).await?;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| broke_off(&error, "the signature's download broke off"))?
    {
        body.extend_from_slice(&chunk);
        if body.len() > SIGNATURE_MAX {
            return Err(Failed("the signature is too large"));
        }
    }
    let text = String::from_utf8(body).map_err(|_bytes| Failed("the signature is not text"))?;
    Ok(text.trim().to_owned())
}

/// The installer at `url`, each chunk handed to `write` as it arrives (the attempt's file, through
/// its one handle). Returns the bytes written; refuses past [`INSTALLER_MAX`].
pub(super) async fn installer(
    client: &reqwest::Client,
    url: &Url,
    deadline: Instant,
    mut write: impl FnMut(&[u8]) -> std::io::Result<()>,
) -> Result<u64, Failed> {
    let mut response = get(client, url, deadline).await?;
    let mut written = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| broke_off(&error, "the installer's download broke off"))?
    {
        written = written.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if written > INSTALLER_MAX {
            return Err(Failed("the installer is too large"));
        }
        write(&chunk).map_err(|_io| Failed("the installer was not saved"))?;
    }
    Ok(written)
}

/// The line for a body that stopped: past the deadline, or `line`.
fn broke_off(error: &reqwest::Error, line: &'static str) -> Failed {
    Failed(if error.is_timeout() {
        PAST_DEADLINE
    } else {
        line
    })
}

/// The line for a download past its deadline.
const PAST_DEADLINE: &str = "the download took longer than its deadline";

/// A GET of `url` that answered a success status, the whole request (connection, headers and
/// body) bounded by `deadline`.
async fn get(
    client: &reqwest::Client,
    url: &Url,
    deadline: Instant,
) -> Result<reqwest::Response, Failed> {
    let left = deadline.saturating_duration_since(crate::open::now());
    if left.is_zero() {
        return Err(Failed(PAST_DEADLINE));
    }
    let response = client
        .get(url.clone())
        .timeout(left)
        .send()
        .await
        .map_err(|error| {
            Failed(if error.is_timeout() {
                PAST_DEADLINE
            } else if error.is_connect() {
                "the release could not be reached"
            } else {
                "the request failed"
            })
        })?;
    if !response.status().is_success() {
        return Err(Failed("the release answered an error status"));
    }
    Ok(response)
}
