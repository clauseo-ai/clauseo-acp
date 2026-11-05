//! The credentials file a credential link's entry was written on (S15 `PR3b` round 4, L5).
//!
//! The link sits in the state folder, which the agent can write: after a launch it can swap the
//! link for another name for another file. A take-back through the link's path would then
//! remove the entry from that other file and leave it, and the Low label, on the credentials
//! file. So the launch records the credentials file, by its path and file ID, before the entry
//! is written, and the way back opens that path as itself and checks the ID instead
//! (`apply/back.rs`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::file::{Slot, State};
use super::{EntryId, EntryState};
use crate::windows::writer::FileId;

/// A file as a launch found it: the path it was named by and its file ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RecordedFile {
    /// Its path, outside the state folder (N8).
    pub path: PathBuf,
    /// Its ID when it was recorded.
    pub id: FileId,
}

impl RecordedFile {
    /// `path`, with the ID its file had when read.
    pub fn new(path: PathBuf, id: FileId) -> RecordedFile {
        RecordedFile { path, id }
    }
}

/// Records `file` on entry `id` when it is still pending with none recorded; whether it did.
pub(super) fn set_file(state: &mut State, id: EntryId, file: &RecordedFile) -> bool {
    let mut changed = false;
    for slot in &mut state.slots {
        if let Slot::Known(known) = slot
            && known.entry.id == id
            && known.entry.state == EntryState::Pending
            && known.entry.file.is_none()
        {
            known.entry.file = Some(file.clone());
            changed = true;
        }
    }
    changed
}
