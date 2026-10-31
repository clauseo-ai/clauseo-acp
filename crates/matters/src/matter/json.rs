//! `.clauseo/matter.json`: the matter's id and creation time, inside the folder, so a moved
//! folder can be found again. Version 1:
//! `{"version": 1, "matter_id": "<uuid>", "created_at": <ms since the epoch>}`.
//!
//! Readers accept any `version` and ignore keys they do not know; they need `matter_id` and
//! `created_at`. A format that drops either uses a new file name.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clauseo_protocol::{MatterId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::{Error, MissingReason, fsync, write};

/// The folder inside the matter folder that holds `matter.json`; excluded from every checkpoint.
pub(crate) const DIR: &str = ".clauseo";
/// The file's name.
const FILE: &str = "matter.json";
/// The version this crate writes.
const VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct MatterJson {
    #[serde(default)]
    version: u32,
    matter_id: MatterId,
    created_at: Timestamp,
}

/// `<folder>/.clauseo/matter.json`.
pub(crate) fn path(folder: &Path) -> PathBuf {
    folder.join(DIR).join(FILE)
}

/// Writes `matter.json` for matter `id` into `folder` by temp file and rename, making
/// `.clauseo` if needed and hiding it on Windows.
pub(crate) fn write(folder: &Path, id: MatterId, created_at: Timestamp) -> Result<(), Error> {
    let dir = folder.join(DIR);
    if !dir.is_dir() {
        write::create_dir_all(&dir).map_err(|err| Error::io("create", &dir, err))?;
        fsync::sync_dir(folder).map_err(|err| Error::io("sync", folder, err))?;
    }
    write::hide_dir(&dir);
    let json = MatterJson {
        version: VERSION,
        matter_id: id,
        created_at,
    };
    let bytes = serde_json::to_vec(&json).map_err(|err| Error::history("matter.json", err))?;
    let target = path(folder);
    write::replace(&target, &mut |file| {
        file.write_all(&bytes)
            .map_err(|err| Error::io("write", &target, err))
    })
}

/// Hides `folder`'s `.clauseo` again on Windows, where a zip or a sync can drop the attribute;
/// logged, never an error.
pub(crate) fn hide(folder: &Path) {
    write::hide_dir(&folder.join(DIR));
}

/// What the marker check finds in a folder: the one reading of `.clauseo/matter.json` that
/// opening, every checkpoint, import, locate and relink share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Found {
    /// The folder does not exist, or is not a folder.
    NoFolder,
    /// The folder has no `matter.json`.
    None,
    /// A file that does not read as `matter.json`.
    Unreadable,
    /// A matter's id.
    Matter(MatterId),
}

impl Found {
    /// The [`Error::FolderMissing`] for matter `id` in `folder`, or `Ok` when the marker names it.
    pub(crate) fn expect(self, folder: &Path, id: MatterId) -> Result<(), Error> {
        let reason = match self {
            Self::Matter(found) if found == id => return Ok(()),
            Self::Matter(other) => MissingReason::OtherMatter(other),
            Self::NoFolder => MissingReason::NoFolder,
            Self::None => MissingReason::NoMatterJson,
            Self::Unreadable => MissingReason::Unreadable,
        };
        Err(Error::FolderMissing {
            folder: folder.to_owned(),
            reason,
        })
    }
}

/// Reads `folder`'s marker. A read that fails for any reason but a missing file (permissions,
/// an offline cloud placeholder) is an [`Error::Io`], not a finding.
pub(crate) fn read(folder: &Path) -> Result<Found, Error> {
    match std::fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Ok(Found::NoFolder),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Found::NoFolder),
        Err(err) => return Err(Error::io("stat", folder, err)),
    }
    let file = path(folder);
    match std::fs::read(&file) {
        Ok(bytes) => Ok(serde_json::from_slice::<MatterJson>(&bytes)
            .map_or(Found::Unreadable, |json| Found::Matter(json.matter_id))),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Found::None),
        Err(err) => Err(Error::io("read", file, err)),
    }
}

/// Checks that `folder` exists and its `matter.json` names matter `id`.
pub(crate) fn check(folder: &Path, id: MatterId) -> Result<(), Error> {
    read(folder)?.expect(folder, id)
}

/// Removes `matter.json`, and `.clauseo` if that leaves it empty; for a cancelled or failed
/// import. Failures are logged, not returned: the import's own result is what matters.
pub(crate) fn remove(folder: &Path) {
    let file = path(folder);
    if let Err(err) = write::remove_file(&file)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(path = %file.display(), error = %err, "matter.json left behind");
    }
    let dir = folder.join(DIR);
    if let Err(err) = write::remove_dir(&dir)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::debug!(path = %dir.display(), error = %err, ".clauseo kept");
    }
}
