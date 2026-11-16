//! The history's copy of a file a write in place overwrites ([`write::SavedCopy`], macOS; Linux
//! in unit tests): confirmed against the open file before the write, and written back through
//! it if the write fails part way.

use std::fs::File;
use std::io::{self, Read as _};

use clauseo_protocol::MatterPath;

use crate::checkpoint::Core;
use crate::history::{HistoryStore as _, Oid};
use crate::write;

/// The history's copy of the matter's `path`, for its write in place
/// ([`write::Staged::rename_or_in_place`]): `blob` once [`confirm_saved`] confirmed it.
pub(super) struct Saved<'a> {
    pub(super) core: &'a Core,
    pub(super) path: &'a MatterPath,
    pub(super) blob: Option<Oid>,
}

impl write::SavedCopy for Saved<'_> {
    fn confirm(&mut self, file: &mut File) -> bool {
        self.blob = confirm_saved(self.core, self.path, file);
        self.blob.is_some()
    }

    fn write_back(&mut self, file: &mut File) -> io::Result<u64> {
        let blob = self
            .blob
            .ok_or_else(|| io::Error::other("no confirmed version"))?;
        self.core
            .history
            .read_blob(blob, self.path.as_str(), file)
            .map_err(io::Error::other)
    }
}

/// The blob the history holds of what the open target `file` (the matter's `path`) has now,
/// before a write in place overwrites it ([`write::Staged::rename_or_in_place`]): the safety
/// net for that write, which is not atomic, and what puts the file back if it fails part way.
/// All of:
/// - the stat cache has the file in the last checkpoint's tree, as a blob, with the stat the open
///   file has now (nothing wrote to it since that checkpoint);
/// - that blob reads back whole from the history, its bytes hashed to its id (`read_blob`);
/// - the open file's bytes hash to that same id.
///
/// Any failure is `None`, "not confirmed": the file is not written in place, and is named as
/// refused.
pub(super) fn confirm_saved(core: &Core, path: &MatterPath, file: &mut File) -> Option<Oid> {
    let entry = core.cache.get(path.as_str())?;
    let blob = entry.state.blob()?;
    let metadata = file.metadata().ok()?;
    if crate::stat::Stat::from_metadata(&metadata) != entry.stat {
        return None;
    }
    core.history
        .read_blob(blob, path.as_str(), &mut io::sink())
        .ok()?;
    blob_id(file, metadata.len())
        .is_ok_and(|found| found == blob)
        .then_some(blob)
}

/// The git blob id of the `len` bytes of the open `file`, read from its start (its position).
fn blob_id(file: &mut File, len: u64) -> io::Result<Oid> {
    let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
    hasher.update(format!("blob {len}\0").as_bytes());
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut read_total = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
        read_total += u64::try_from(read).unwrap_or(u64::MAX);
    }
    if read_total != len {
        return Err(io::Error::other("the file changed while it was read"));
    }
    let id = hasher.try_finalize().map_err(io::Error::other)?;
    <[u8; 20]>::try_from(id.as_bytes())
        .map(Oid)
        .map_err(io::Error::other)
}
