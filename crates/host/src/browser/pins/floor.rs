//! The floor record: the `published` and `version` of the newest manifest the host ever stored,
//! kept in its own host-owned file beside the stored pair (`<data dir>/browser-pins/`,
//! [`FLOOR_FILE`]). The pair's signature is what makes it trusted; this file only remembers how
//! high it was, so a stored pair that stops verifying (a disk fault, a half-written store, a
//! deleted file) does not drop the app back to the lower bundled copy. Readers refuse every copy
//! until one at or above the floor is there ([`Floor::covered_by`]), and the refresh stores only
//! such a one.
//!
//! It is written before the pair it describes, and only raised, never lowered ([`raise`]). A
//! floor file that is there but unreadable makes readers refuse every copy too; the next refresh
//! that takes a manifest (stored or the same bytes again) writes it afresh.

use std::path::Path;

use clauseo_blocklist::Generated;
use clauseo_protocol::Timestamp;
use serde::{Deserialize, Serialize};

use super::{Pins, read_capped};
use crate::write;

/// The floor record's file name in the stored pair's folder.
pub const FLOOR_FILE: &str = "browser-pins-floor-v1.json";

/// The largest floor file read: a real one is under 100 bytes.
const MAX_FLOOR_BYTES: u64 = 4 * 1024;

/// The file's form.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FloorFile {
    /// The manifest's signed `published`, RFC 3339.
    published: String,
    /// Its `stable.version`, dotted digits.
    version: String,
}

/// The newest manifest the host stored: its date and version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    published: Generated,
    version: String,
    numbers: Vec<u64>,
}

impl Floor {
    /// The floor `pins` set.
    pub fn of(pins: &Pins) -> Self {
        Self {
            published: pins.published,
            version: pins.version.clone(),
            numbers: pins.numbers.clone(),
        }
    }

    /// When the manifest that set it was published.
    pub fn published(&self) -> Generated {
        self.published
    }

    /// Whether `pins` stand at or above it: published no earlier, and a version no lower,
    /// compared number by number over the whole version.
    pub fn covered_by(&self, pins: &Pins) -> bool {
        pins.published >= self.published && pins.numbers >= self.numbers
    }

    /// Whether it is dated more than a day ahead of `now`.
    pub fn is_ahead_of(&self, now: Timestamp) -> bool {
        self.published.unix_seconds() > super::seconds(now) + crate::blocklist::DAY
    }
}

/// Why the floor file could not be used. Never names a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreadable;

/// The floor in `folder`: `Ok(None)` when there is no file, `Err` when it is there but could not
/// be read or parsed.
pub fn read(folder: &Path) -> Result<Option<Floor>, Unreadable> {
    let bytes = match read_capped(&folder.join(FLOOR_FILE), MAX_FLOOR_BYTES) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Unreadable),
    };
    let file: FloorFile = serde_json::from_slice(&bytes).map_err(|_unsaid| Unreadable)?;
    let published = Generated::parse(&file.published).ok_or(Unreadable)?;
    let numbers = crate::browser::chrome::version(&file.version).ok_or(Unreadable)?;
    Ok(Some(Floor {
        published,
        version: file.version,
        numbers,
    }))
}

/// Raises the floor in `folder` (created if missing) to `pins`: written when there is no floor
/// file, when it is unreadable, or when `pins` stand above it and differ from it; else left.
pub fn raise(folder: &Path, pins: &Pins) -> std::io::Result<()> {
    let new = Floor::of(pins);
    if let Ok(Some(old)) = read(folder)
        && (old == new || !old.covered_by(pins))
    {
        return Ok(());
    }
    let bytes = serde_json::to_vec(&FloorFile {
        published: pins.published.as_str().to_owned(),
        version: new.version,
    })
    .map_err(std::io::Error::other)?;
    write::create_dir_all(folder)?;
    let path = folder.join(FLOOR_FILE);
    write::sweep_temp_files(&path);
    write::replace_private_file(&path, &bytes)?;
    write::sync_dir(folder)
}
