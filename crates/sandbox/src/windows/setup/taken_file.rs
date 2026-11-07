//! `<data_dir>\sandbox\reclaim-taken.json` (S15 `PR4i` follow-up): the items `--reclaim` took,
//! each with the matter it was walked for, its final path and its file ID, so the app strips the
//! denies an agent left of the lawyer on those items and no others (`reclaim::hand_back_after`).
//! The exe writes it as the owner (`Owner::run_as`) beside `setup-result.json`, through the same
//! checks; it never writes the ledger (K1). `UserSetup::reclaim` removes it before the run, reads
//! it after through one owner-checked handle, records the items in the ledger, and removes it.
//!
//! The file is the lawyer's, so what it says lets the app do nothing the lawyer couldn't: the
//! strip touches only an item the lawyer owns, at that path with that file ID, inside a tree the
//! ledger owes a clean-up. Pure apart from the file's read and write: tested on every OS.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::windows::matters::SANDBOX_DIR;
use crate::windows::reclaim::TakenItem;
use crate::windows::write;

/// The file's name, in `<data_dir>\sandbox\`.
pub const TAKEN_FILE: &str = "reclaim-taken.json";

/// The most items one run lists; any more are taken but left out, and keep only the strip of
/// other pool users' allows.
pub const TAKEN_CAP: usize = 10_000;

/// The most the file may hold: far more than [`TAKEN_CAP`] items.
#[cfg(any(windows, test))]
const FILE_CAP: u64 = 16 * 1024 * 1024;

/// The file's contents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Taken {
    taken: Vec<TakenItem>,
}

/// The file's path under `data_dir`.
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(SANDBOX_DIR).join(TAKEN_FILE)
}

/// Writes the first [`TAKEN_CAP`] of `taken` atomically, as `result_file::write` writes: the
/// folder made if missing, the old file checked first and the new one after.
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "the exe that writes it is Windows only")
)]
pub(crate) fn write(data_dir: &Path, taken: &[TakenItem]) -> io::Result<()> {
    let file = path(data_dir);
    let listed = Taken {
        taken: taken.iter().take(TAKEN_CAP).cloned().collect(),
    };
    let mut bytes = serde_json::to_vec(&listed).map_err(io::Error::other)?;
    bytes.push(b'\n');
    if let Some(folder) = file.parent() {
        write::make_sandbox_dir(folder)?;
    }
    write::check_owned_file(&file)?;
    write::write_atomic(&file, &bytes)?;
    write::check_owned_file(&file)
}

/// The items listed, through the one handle its checks go through (`write::read_owned_file`):
/// none when the file is missing; why when it can't be read or parsed, is a link, has a second
/// hard link, or is too large.
#[cfg(any(windows, test))]
pub(crate) fn read(data_dir: &Path) -> Result<Vec<TakenItem>, String> {
    match write::read_owned_file(&path(data_dir), FILE_CAP) {
        Ok(Some(bytes)) => serde_json::from_slice::<Taken>(&bytes)
            .map(|listed| listed.taken)
            .map_err(|error| error.to_string()),
        Ok(None) => Ok(Vec::new()),
        Err(error) => Err(error.to_string()),
    }
}

/// Removes the file, so a run that writes none isn't read as the last one's. Missing is fine.
#[cfg(any(windows, test))]
pub(crate) fn remove(data_dir: &Path) -> io::Result<()> {
    write::remove_owned_file(&path(data_dir))
}
