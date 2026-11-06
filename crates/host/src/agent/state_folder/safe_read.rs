//! Reading a file in a matter's state folder, which the agent can plant (R17 PR-C): never
//! through a link, and never a file with a second name, which could be a hard link to a
//! credential. Used for Claude's `.claude.json` and `settings.json` and Codex's `config.toml`
//! before they are rewritten.

use std::io::{self, Read as _};
use std::path::Path;

/// The largest file read; a larger one is refused (Clauseo's own files are a few hundred bytes,
/// Claude's `.claude.json` a few kilobytes).
pub(crate) const MAX_LEN: u64 = 16 * 1024 * 1024;

/// What [`read_plain`] found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Found {
    /// Nothing at the name.
    Missing,
    /// A link, a folder, a file with more than one name or one over [`MAX_LEN`]: not read.
    Refused,
    /// A plain file with one name, and its bytes.
    Bytes(Vec<u8>),
}

/// Reads the file `path` only when it is a plain file with one name: opened with `O_NOFOLLOW`
/// on Unix and checked on the open file (`fstat`), so a name swapped meanwhile is not followed;
/// on Windows its link count is read first (`CreateFileW` with access 0 and
/// `OPEN_REPARSE_POINT`) and it is opened with `OPEN_REPARSE_POINT`, its attributes checked on
/// the open file. Err for any other failure.
pub(crate) fn read_plain(path: &Path) -> io::Result<Found> {
    #[cfg(windows)]
    match super::file_id::link_count(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Found::Missing),
        Err(error) => return Err(error),
        Ok(None) => return Ok(Found::Refused),
        Ok(Some(links)) if links != 1 => return Ok(Found::Refused),
        Ok(Some(_)) => {}
    }
    let file = match crate::write::open_read_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Found::Missing),
        Err(error) if is_link_refusal(&error) => return Ok(Found::Refused),
        Err(error) => return Err(error),
    };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_LEN || !one_name(&meta) {
        return Ok(Found::Refused);
    }
    let mut bytes = Vec::new();
    file.take(MAX_LEN + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_LEN) {
        return Ok(Found::Refused);
    }
    Ok(Found::Bytes(bytes))
}

/// Whether an open's failure is `O_NOFOLLOW` meeting a link (`ELOOP`, and `EMLINK` on FreeBSD).
fn is_link_refusal(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        let raw = error.raw_os_error();
        raw == Some(rustix::io::Errno::LOOP.raw_os_error())
            || raw == Some(rustix::io::Errno::MLINK.raw_os_error())
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

/// Whether the open file has one name and is no link. On Windows the name count was read before
/// the open, and a reparse point opened as itself shows in its attributes.
fn one_name(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        meta.nlink() == 1
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        /// `FILE_ATTRIBUTE_REPARSE_POINT`.
        const REPARSE: u32 = 0x400;
        meta.file_attributes() & REPARSE == 0
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = meta;
        false
    }
}
