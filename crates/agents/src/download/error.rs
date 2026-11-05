//! How a download failed: [`FetchError`], and the plain functions that sort an error into the
//! decision record's kinds (offline, blocked, disk full, failed), each unit-tested.

use std::io;
use std::path::Path;

use crate::log::{redact, redact_url};

/// Which way a download failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FetchKind {
    /// Name resolution failed, or connect failed with no route: wait and try again.
    Offline,
    /// A 403 or 407, a TLS failure, or a web page where a file should be: a proxy or a firewall
    /// is in the way, and a retry would meet it again.
    Blocked,
    /// The disk filled while writing.
    DiskFull,
    /// The file's digest did not match; the file is deleted.
    Integrity,
    /// The job was paused; the `.part` file is kept for the next try.
    Cancelled,
    /// Anything else.
    Failed,
}

/// A failed download: its kind, the host it went to (with no user name or password), the HTTP
/// status when there was one, and one short English line, redacted for the log.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{detail}")]
pub(crate) struct FetchError {
    /// Which way it failed.
    pub(crate) kind: FetchKind,
    /// The host (`host[:port]`), with any `user:pass@` dropped; `None` with no network involved.
    pub(crate) host: Option<String>,
    /// The HTTP status, when a server answered.
    pub(crate) status: Option<u16>,
    /// One line for the log, passed through [`redact`].
    pub(crate) detail: String,
}

impl FetchError {
    /// An error with `detail` redacted against `data_dir`.
    pub(crate) fn new(
        kind: FetchKind,
        host: Option<&str>,
        status: Option<u16>,
        detail: &str,
        data_dir: &Path,
    ) -> FetchError {
        FetchError {
            kind,
            host: host.map(str::to_owned),
            status,
            detail: redact(detail, data_dir),
        }
    }
}

/// The kind of a ureq error, from the request or from reading the body.
pub(crate) fn classify(err: &ureq::Error) -> FetchKind {
    match err {
        ureq::Error::HostNotFound
        | ureq::Error::Timeout(ureq::Timeout::Connect | ureq::Timeout::Resolve) => {
            FetchKind::Offline
        }
        ureq::Error::Io(io) => classify_io(io),
        ureq::Error::Tls(_) | ureq::Error::Pem(_) | ureq::Error::Rustls(_) => FetchKind::Blocked,
        ureq::Error::ConnectProxyFailed(reason) => match proxy_status(reason) {
            Some(status) => status_kind(status),
            None => FetchKind::Failed,
        },
        _ => FetchKind::Failed,
    }
}

/// The HTTP status of a ureq error, when it carries one: a refused proxy `CONNECT`.
pub(crate) fn error_status(err: &ureq::Error) -> Option<u16> {
    match err {
        ureq::Error::StatusCode(status) => Some(*status),
        ureq::Error::ConnectProxyFailed(reason) => proxy_status(reason),
        _ => None,
    }
}

/// The kind of an io error met on the network: no route is offline, a TLS failure (rustls
/// reports it as an io error wrapping its own) is blocked, the rest failed. A refused
/// connection is a host that answers, so it failed rather than offline.
pub(crate) fn classify_io(err: &io::Error) -> FetchKind {
    if err
        .get_ref()
        .is_some_and(|inner| inner.downcast_ref::<rustls::Error>().is_some())
    {
        return FetchKind::Blocked;
    }
    match err.kind() {
        io::ErrorKind::NetworkUnreachable
        | io::ErrorKind::HostUnreachable
        | io::ErrorKind::NetworkDown => FetchKind::Offline,
        _ => FetchKind::Failed,
    }
}

/// The kind of an error writing the file: a full disk, or failed.
pub(crate) fn write_kind(err: &io::Error) -> FetchKind {
    if err.kind() == io::ErrorKind::StorageFull {
        FetchKind::DiskFull
    } else {
        FetchKind::Failed
    }
}

/// The kind of an HTTP status that is not the file: 403 and 407 are a proxy or firewall.
pub(crate) fn status_kind(status: u16) -> FetchKind {
    match status {
        403 | 407 => FetchKind::Blocked,
        _ => FetchKind::Failed,
    }
}

/// The status in ureq's `proxy server responded 407/Proxy Authentication Required`.
fn proxy_status(reason: &str) -> Option<u16> {
    let (_, after) = reason.split_once("responded ")?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// `host[:port]` of `url`, with any `user:pass@` dropped; `None` when it has no `scheme://`.
pub(crate) fn host_of(url: &str) -> Option<String> {
    let clean = redact_url(url);
    let (_, rest) = clean.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest.get(..end)?;
    (!host.is_empty()).then(|| host.to_owned())
}

#[cfg(test)]
mod tests;
