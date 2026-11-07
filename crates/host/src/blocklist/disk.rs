//! The two files on disk: a pair read from a folder (the stored one or the bundled one), and the
//! stored pair's write.
//!
//! The write replaces each file atomically (`write::replace_private_file`: a synced temp file
//! renamed over it), the list first and its signature last, then syncs the folder. A crash
//! between the two renames leaves the new list beside the old signature, which fails
//! verification: start-up then ignores the stored pair (the bundled snapshot serves) and the
//! next fetch writes a good pair again. A mismatched pair is never read as good.

use std::io::{self, Read as _};
use std::path::Path;

use clauseo_blocklist::{LIST_FILE, SIGNATURE_FILE};

use crate::write;

/// The two files' bytes.
pub(crate) struct Pair {
    pub(crate) gz: Vec<u8>,
    pub(crate) signature: Vec<u8>,
}

/// Reads `folder`'s pair, each file capped (`max_list`, `max_signature`). `Ok(None)` when either
/// file is missing.
pub(crate) fn read_pair(
    folder: &Path,
    max_list: u64,
    max_signature: u64,
) -> io::Result<Option<Pair>> {
    let gz = read_capped(&folder.join(LIST_FILE), max_list);
    let signature = read_capped(&folder.join(SIGNATURE_FILE), max_signature);
    if missing(&gz) || missing(&signature) {
        return Ok(None);
    }
    Ok(Some(Pair {
        gz: gz?,
        signature: signature?,
    }))
}

/// Whether `result` failed because the file is not there.
fn missing(result: &io::Result<Vec<u8>>) -> bool {
    matches!(result, Err(error) if error.kind() == io::ErrorKind::NotFound)
}

/// Reads `path`, failing when it holds more than `cap` bytes.
fn read_capped(path: &Path, cap: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(cap.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > cap {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "over the size cap",
        ));
    }
    Ok(bytes)
}

/// Stores `pair` in `folder` (created if missing): the list, then the signature.
pub(crate) fn store_pair(folder: &Path, pair: &Pair) -> io::Result<()> {
    write::create_dir_all(folder)?;
    let list = folder.join(LIST_FILE);
    let signature = folder.join(SIGNATURE_FILE);
    write::sweep_temp_files(&list);
    write::sweep_temp_files(&signature);
    write::replace_private_file(&list, &pair.gz)?;
    write::replace_private_file(&signature, &pair.signature)?;
    write::sync_dir(folder)
}
