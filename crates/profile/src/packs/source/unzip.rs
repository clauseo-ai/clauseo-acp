//! A `.zip` unpacked into a fresh folder, refusing whatever could land outside it or blow up:
//!
//! - every entry's name is UTF-8, `/`-separated (a `\` is refused), relative (no leading `/`,
//!   no drive letter or `:` anywhere), with no empty, `.` or `..` part, and each part a name every
//!   OS can hold (no control character, none of `<>:"|?*`, no trailing `.` or space, no Windows
//!   device name, `COM¹` and `CON .txt` included, no 8.3 short name such as `PROGRA~1`); two
//!   names equal but for case are refused;
//! - no symlink (nor any entry whose Unix mode is neither a file nor a folder: zip has no hard
//!   links), no encrypted entry;
//! - at most [`ENTRIES_MAX`] entries and [`UNPACKED_MAX`] bytes unpacked, counted as the bytes
//!   come out, not as the archive claims; the archive itself at most [`DOWNLOAD_MAX`].
//!
//! Any refusal leaves nothing behind: the folder is removed. Files get the default mode (no exec
//! bit). Names a pack cannot hold (a space, say) are unpacked; the skill holding one is refused
//! when it is read (`added::build`), as a folder's would be. Off the host thread.

use std::collections::BTreeSet;
use std::io::{Cursor, Read as _};
use std::path::Path;

use zip::ZipArchive;

use super::{DOWNLOAD_MAX, ENTRIES_MAX, SourceError, UNPACKED_MAX};
use crate::write;

/// The most bytes an entry's name has.
const NAME_MAX: usize = 400;

/// The most parts an entry's name has.
const DEPTH_MAX: usize = 32;

/// What [`unzip`] unpacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Unpacked {
    /// Files written.
    pub files: usize,
    /// Bytes written, all files together.
    pub bytes: u64,
}

/// Unpacks the archive `bytes` into `into`, which must not exist yet (its parent must).
pub fn unzip(bytes: &[u8], into: &Path) -> Result<Unpacked, SourceError> {
    unzip_capped(bytes, into, (ENTRIES_MAX, UNPACKED_MAX))
}

/// [`unzip`] under the caps `(entries, unpacked bytes)` in place of [`ENTRIES_MAX`] and
/// [`UNPACKED_MAX`] (the archive itself is still at most [`DOWNLOAD_MAX`]).
pub fn unzip_capped(
    bytes: &[u8],
    into: &Path,
    caps: (usize, u64),
) -> Result<Unpacked, SourceError> {
    if bytes.len() as u64 > DOWNLOAD_MAX {
        return Err(SourceError::TooBig);
    }
    write::create_dir(into).map_err(|_io| SourceError::Io)?;
    let result = unpack(bytes, into, caps);
    if result.is_err() {
        write::remove_dir_all(into);
    }
    result
}

/// The unpacking of [`unzip_capped`].
fn unpack(
    bytes: &[u8],
    into: &Path,
    (entries, cap): (usize, u64),
) -> Result<Unpacked, SourceError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_bad| SourceError::Unsafe)?;
    if archive.len() > entries {
        return Err(SourceError::TooBig);
    }
    let mut seen = BTreeSet::new();
    let mut done = Unpacked::default();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_bad| SourceError::Unsafe)?;
        let raw = entry
            .name()
            .map_err(|_bad| SourceError::Unsafe)?
            .into_owned();
        let is_dir = raw.ends_with('/');
        let name = raw.strip_suffix('/').unwrap_or(&raw);
        if !is_plain_name(name) || entry.encrypted() || entry.is_symlink() {
            return Err(SourceError::Unsafe);
        }
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170_000;
            if kind != 0 && kind != 0o100_000 && kind != 0o040_000 {
                return Err(SourceError::Unsafe);
            }
        }
        if !seen.insert(name.to_lowercase()) && !is_dir {
            return Err(SourceError::Unsafe);
        }
        let target = into.join(name);
        if is_dir || entry.is_dir() {
            write::create_dir_all(&target).map_err(|_io| SourceError::Io)?;
            continue;
        }
        let left = cap - done.bytes;
        let mut out = Vec::new();
        (&mut entry)
            .take(left + 1)
            .read_to_end(&mut out)
            .map_err(|_bad| SourceError::Unsafe)?;
        if out.len() as u64 > left {
            return Err(SourceError::TooBig);
        }
        done.bytes += out.len() as u64;
        done.files += 1;
        // A new file only: a name met before (in any case, on a disk that ignores it) fails.
        write::write_new(&target, &out).map_err(|_io| SourceError::Unsafe)?;
    }
    Ok(done)
}

/// Whether an entry's `name` (its trailing `/` taken off) is relative and plain.
fn is_plain_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split('/').collect();
    !name.is_empty()
        && name.len() <= NAME_MAX
        && parts.len() <= DEPTH_MAX
        && !name.contains(['\\', ':'])
        && parts.iter().all(|part| is_plain_part(part))
}

/// Whether one part of a name is a file or folder name every OS can hold.
pub(super) fn is_plain_part(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && !part.ends_with(['.', ' '])
        && !part
            .chars()
            .any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
        && !is_device(part)
        && !is_short_alias(part)
}

/// Whether `part` looks like a Windows 8.3 short name (`PROGRA~1`, `x~12.txt`): a `~` then
/// digits, then the end or a `.`. Such a name can stand for another file on the same disk.
fn is_short_alias(part: &str) -> bool {
    part.match_indices('~').any(|(at, _)| {
        let rest = part.get(at + 1..).unwrap_or_default();
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        digits > 0 && matches!(rest.as_bytes().get(digits), None | Some(b'.'))
    })
}

/// Whether `part` names a Windows device (`con`, `nul`, `com1`, `lpt9`, `com¹`, ...), any
/// case, with or without an extension, spaces before the extension ignored (`CON .txt`), as
/// Windows reads it.
fn is_device(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_lowercase();
    let numbered = |prefix: &str| {
        stem.strip_prefix(prefix).is_some_and(|rest| {
            let mut chars = rest.chars();
            let digit = chars
                .next()
                .is_some_and(|c| c.is_ascii_digit() || matches!(c, '¹' | '²' | '³'));
            digit && chars.next().is_none()
        })
    };
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul") || numbered("com") || numbered("lpt")
}
