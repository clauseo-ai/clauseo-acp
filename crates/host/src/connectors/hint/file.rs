//! `<data_dir>/connectors/hints.json`, the matters where the case-law hint was shown or
//! dismissed: `{"v":1,"matters":{"<matter id>":"shown"|"dismissed"}}`. Neither mark shows the
//! hint again in that matter. Written whole and atomically (`write::replace_private_file`).
//!
//! Read once, at the first need. A file that is missing, does not read, or is of another
//! version counts as no marks (logged), so the worst a damaged file does is offer the hint once
//! more; a mark of a value not known (a later version's) counts as a mark and is kept as it is.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};

use crate::log_file::Cause;

/// The file's name in the connectors folder.
pub(crate) const FILE: &str = "hints.json";
/// The connectors folder in the data folder.
const FOLDER: &str = "connectors";
/// The version this host writes.
const VERSION: u32 = 1;

/// One matter's mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    /// The hint was offered.
    Shown,
    /// The lawyer dismissed it.
    Dismissed,
}

impl Mark {
    fn as_str(self) -> &'static str {
        match self {
            Self::Shown => "shown",
            Self::Dismissed => "dismissed",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    v: u32,
    #[serde(default)]
    matters: BTreeMap<String, String>,
}

/// The hint file and its marks, read at the first need.
#[derive(Debug)]
pub(crate) struct Hints {
    path: PathBuf,
    marks: Option<BTreeMap<String, String>>,
}

impl Hints {
    /// The file in `data_dir`'s connectors folder; nothing is read yet.
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(FOLDER).join(FILE),
            marks: None,
        }
    }

    /// Whether `matter` has a mark (shown or dismissed).
    pub(crate) fn marked(&mut self, matter: MatterId) -> bool {
        self.marks().contains_key(&matter.to_string())
    }

    /// Marks `matter` and writes the file. The mark holds in memory even when the write fails
    /// (the error is returned for the log), so the hint still shows once per matter this run.
    pub(crate) fn mark(&mut self, matter: MatterId, mark: Mark) -> io::Result<()> {
        let marks = self.marks();
        marks.insert(matter.to_string(), mark.as_str().to_owned());
        let disk = OnDisk {
            v: VERSION,
            matters: marks.clone(),
        };
        let bytes = serde_json::to_vec(&disk).map_err(io::Error::other)?;
        let folder = self.path.parent().unwrap_or(&self.path);
        crate::write::create_dir_all(folder)?;
        crate::write::replace_private_file(&self.path, &bytes)
    }

    fn marks(&mut self) -> &mut BTreeMap<String, String> {
        let path = &self.path;
        self.marks.get_or_insert_with(|| read(path))
    }
}

/// The marks on disk; none when the file is missing or unusable.
fn read(path: &Path) -> BTreeMap<String, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return BTreeMap::new(),
        Err(err) => {
            tracing::warn!(error = %Cause(&err), "connectors: hints.json does not read; no marks");
            return BTreeMap::new();
        }
    };
    match serde_json::from_slice::<OnDisk>(&bytes) {
        Ok(disk) if disk.v == VERSION => disk.matters,
        Ok(disk) => {
            tracing::warn!(
                version = disk.v,
                "connectors: hints.json is of another version; no marks"
            );
            BTreeMap::new()
        }
        Err(_unsaid) => {
            tracing::warn!("connectors: hints.json does not parse; no marks");
            BTreeMap::new()
        }
    }
}

#[cfg(test)]
mod tests;
