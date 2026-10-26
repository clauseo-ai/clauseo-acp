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

use crate::{Error, MissingReason, write};

/// The folder inside the matter folder that holds `matter.json`; excluded from every checkpoint.
pub(crate) const DIR: &str = ".clauseo";
/// The file's name.
const FILE: &str = "matter.json";
/// The version this crate writes.
const VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct MatterJson {
    version: u32,
    matter_id: MatterId,
    created_at: Timestamp,
}

/// `<folder>/.clauseo/matter.json`.
pub(crate) fn path(folder: &Path) -> PathBuf {
    folder.join(DIR).join(FILE)
}

/// Writes `matter.json` for matter `id` into `folder` by temp file and rename, making
/// `.clauseo` if needed.
pub(crate) fn write(folder: &Path, id: MatterId, created_at: Timestamp) -> Result<(), Error> {
    let dir = folder.join(DIR);
    write::create_dir_all(&dir).map_err(|err| Error::io("create", &dir, err))?;
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

/// Checks that `folder` exists and its `matter.json` names matter `id`.
pub(crate) fn check(folder: &Path, id: MatterId) -> Result<(), Error> {
    let missing = |reason| Error::FolderMissing {
        folder: folder.to_owned(),
        reason,
    };
    match std::fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(missing(MissingReason::NoFolder)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(missing(MissingReason::NoFolder));
        }
        Err(err) => return Err(Error::io("stat", folder, err)),
    }
    let file = path(folder);
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(missing(MissingReason::NoMatterJson));
        }
        Err(err) => return Err(Error::io("read", file, err)),
    };
    let Ok(json) = serde_json::from_slice::<MatterJson>(&bytes) else {
        return Err(missing(MissingReason::Unreadable));
    };
    if json.matter_id == id {
        Ok(())
    } else {
        Err(missing(MissingReason::OtherMatter(json.matter_id)))
    }
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
