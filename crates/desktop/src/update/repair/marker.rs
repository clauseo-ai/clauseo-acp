//! The `repair-pending` marker, `<data folder>/repair-pending.json`: written just before the
//! installer starts (`{"version":"0.4.1","at":<unix seconds>}`), read and deleted at the next
//! start ([`consume`]). A fresh one for this version tells the window it came back from the
//! installer: while the admin copy is still missing, the card says the installer didn't finish.
//! With the copy there nothing more is needed: the installer's elevated `--adopt` ran the Safe
//! workspace's set-up itself.

#![expect(
    clippy::disallowed_methods,
    reason = "the marker's write and delete, one small file in the data folder; the desktop crate \
              has no write module"
)]

use std::io;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::super::install::LIFECYCLE;

/// How long a marker counts: past it, the start is not the installer's.
pub(super) const FRESH: Duration = Duration::from_mins(30);

/// The largest marker read.
const MAX: u64 = 4 * 1024;

/// The marker's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Marker {
    /// The version that ran the installer.
    pub(super) version: String,
    /// When, in Unix seconds.
    pub(super) at: u64,
}

/// Writes the marker for `version` at `now` (Unix seconds) to `path`, replacing any.
pub(super) fn write(path: &Path, version: &str, now: u64) -> io::Result<()> {
    let marker = Marker {
        version: version.to_owned(),
        at: now,
    };
    let bytes = serde_json::to_vec(&marker).map_err(io::Error::other)?;
    std::fs::write(path, bytes)
}

/// Deletes the marker at `path`; one that is not there is fine.
pub(super) fn remove(path: &Path) {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            tracing::warn!(target: LIFECYCLE, kind = %error.kind(), "repair: the marker was not deleted");
        }
        _ => {}
    }
}

/// Reads and deletes the marker at `path`, whatever it holds: true when it was for `version`,
/// written no more than [`FRESH`] before `now` (Unix seconds). One line in host.log when there
/// was one.
pub(super) fn consume(path: &Path, version: &str, now: u64) -> bool {
    let read = read(path);
    if matches!(&read, Err(error) if error.kind() == io::ErrorKind::NotFound) {
        return false;
    }
    remove(path);
    let verdict = match read {
        Ok(marker) if marker.version != version => "another version's",
        Ok(marker) if marker.at > now => "from the future",
        Ok(marker) if now - marker.at > FRESH.as_secs() => "expired",
        Ok(_) => "fresh",
        Err(_) => "unreadable",
    };
    tracing::info!(target: LIFECYCLE, verdict, "repair: the installer's marker was read and deleted");
    verdict == "fresh"
}

/// The marker at `path`, read whole when it is no larger than [`MAX`].
fn read(path: &Path) -> io::Result<Marker> {
    use std::io::Read as _;
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "too large"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
