//! The seam to the network: [`Upload`], which desktop implements with `reqwest` (tokio stays out
//! of this crate and of `host`), and [`send_due`], which makes the POSTs that are due.

use std::fmt;

use futures::future::{BoxFuture, Either, select};

use crate::Counts;
use crate::schema::Time;
use crate::sender::Outcome;

/// Where a body goes, under the endpoint desktop holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UploadPath {
    /// `/v1/counts`: a batch of day reports.
    Counts,
    /// `/v1/tally`: one weekly tally.
    Tally,
    /// `/v1/crash`: one crash report (S22).
    Crash,
}

impl UploadPath {
    /// The path, as it goes after the endpoint.
    pub const fn as_str(self) -> &'static str {
        match self {
            UploadPath::Counts => "/v1/counts",
            UploadPath::Tally => "/v1/tally",
            UploadPath::Crash => "/v1/crash",
        }
    }
}

/// A POST that got no status: the network failed, timed out or was refused. It carries no text,
/// so nothing from the network reaches the counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadError;

impl fmt::Display for UploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the upload got no answer")
    }
}

impl std::error::Error for UploadError {}

/// Sends one gzip body (`Content-Encoding: gzip`, `Content-Type: application/json`) and returns
/// the status. Desktop's implementation posts over HTTPS only, follows the OS proxy and never the
/// agent's, sends no cookies and follows no redirect.
pub trait Upload: Send + Sync {
    /// POSTs `gzip_body` to `path`.
    fn post(&self, path: UploadPath, gzip_body: Vec<u8>)
    -> BoxFuture<'_, Result<u16, UploadError>>;
}

/// Makes the POST due at `now`, if any, and passes its answer back. `effective_on` is read
/// before anything is due and again just before the POST: when it says off, counts turn off
/// (deleting the folder) and nothing is sent. Turning counts off while the POST is in flight
/// cancels it, and it is never retried.
pub async fn send_due(
    counts: &Counts,
    upload: &dyn Upload,
    now: Time,
    effective_on: &(dyn Fn() -> bool + Sync),
) {
    if !effective_on() {
        // A failed delete is tried again on the next tick.
        drop(counts.set_enabled(false));
        return;
    }
    for post in counts.due(now) {
        if !effective_on() {
            drop(counts.set_enabled(false));
            return;
        }
        let (id, path, body, cancelled) = post.into_parts();
        match select(upload.post(path, body), cancelled).await {
            Either::Left((Ok(status), _)) => counts.sent(id, Outcome::Status(status)),
            Either::Left((Err(UploadError), _)) => counts.sent(id, Outcome::Failed),
            // Off while in flight: the POST is dropped here, and its answer never comes.
            Either::Right(((), _)) => {}
        }
    }
}
