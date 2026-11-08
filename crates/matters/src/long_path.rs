//! Long paths on Windows. std adds the verbatim prefix (`\\?\`) to a long absolute path itself
//! (its `maybe_verbatim`), but a crate that calls Win32 with the plain path does not, and without
//! a `longPathAware` manifest that call fails once the path reaches 260 UTF-16 units, whatever
//! the `LongPathsEnabled` setting says. gix-odb's loose writes are such a call: tempfile's
//! `persist` renames the new object with `MoveFileExW`. So the history store does its IO under
//! `history.git` through [`long_path`] of the git dir.
//!
//! The verbatim form turns off Win32's path cleanup (`/`, `.`, `..`, trailing dots and spaces),
//! so the path is made absolute first by `std::path::absolute`, which does that cleanup
//! (`GetFullPathNameW`). Nothing is resolved: a junction or a symlink stays as it is. A path
//! only compared, never opened (the matters root, the refused trees), stays plain.

use std::path::{Path, PathBuf};

/// `\`.
#[cfg(any(windows, test))]
const BACKSLASH: u16 = 0x5C;
/// `?`.
#[cfg(any(windows, test))]
const QUESTION: u16 = 0x3F;
/// `.`.
#[cfg(any(windows, test))]
const DOT: u16 = 0x2E;
/// `:`.
#[cfg(any(windows, test))]
const COLON: u16 = 0x3A;

/// `p` for IO on Windows: absolute and verbatim (`C:\x` is `\\?\C:\x`, `\\srv\share\x` is
/// `\\?\UNC\srv\share\x`), so a call that bypasses std works past 260 UTF-16 units. A path that
/// is already verbatim or a device path (`\\.\`), or one that does not turn absolute, comes back
/// as it is: a plain path still works under 260. Elsewhere `p` as it is.
pub(crate) fn long_path(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
        let Ok(absolute) = std::path::absolute(p) else {
            return p.to_path_buf();
        };
        let wide: Vec<u16> = absolute.as_os_str().encode_wide().collect();
        match verbatim(&wide) {
            Some(long) => PathBuf::from(OsString::from_wide(&long)),
            None => p.to_path_buf(),
        }
    }
    #[cfg(not(windows))]
    {
        p.to_path_buf()
    }
}

/// The verbatim form of the absolute Windows path `absolute`, in UTF-16 units: `\\?\` before a
/// drive path, `\\?\UNC\` in place of a UNC path's `\\`. `None` for a path already verbatim or a
/// device path (`\\?\`, `\\.\`), and for any other form (relative, rooted without a drive),
/// which the caller leaves as it is. Plain arithmetic on the units, so tested on every OS.
#[cfg(any(windows, test))]
pub(crate) fn verbatim(absolute: &[u16]) -> Option<Vec<u16>> {
    const PREFIX: [u16; 4] = [BACKSLASH, BACKSLASH, QUESTION, BACKSLASH];
    match absolute {
        [BACKSLASH, BACKSLASH, QUESTION | DOT, BACKSLASH, ..] => None,
        [BACKSLASH, BACKSLASH, first, ..] if *first != BACKSLASH => {
            let unc = PREFIX.iter().copied().chain("UNC".encode_utf16());
            Some(unc.chain(absolute.iter().skip(1).copied()).collect())
        }
        [drive, COLON, BACKSLASH, ..] if is_ascii_letter(*drive) => {
            Some(PREFIX.iter().chain(absolute).copied().collect())
        }
        _ => None,
    }
}

/// The plain form of a short verbatim path, such as the volume root `GetVolumePathNameW` gives
/// for a verbatim path: `\\?\C:\` is `C:\`, `\\?\UNC\srv\share\` is `\\srv\share\`. Any other
/// form (a volume GUID path `\\?\Volume{..}\`, a plain path) comes back as it is. Only for a
/// path known to be short: the plain form gets Win32's path cleanup and the 260 limit back.
#[cfg(any(windows, test))]
pub(crate) fn plain(path: &[u16]) -> Vec<u16> {
    const PREFIX: [u16; 4] = [BACKSLASH, BACKSLASH, QUESTION, BACKSLASH];
    let Some(rest) = path.strip_prefix(&PREFIX[..]) else {
        return path.to_vec();
    };
    let unc: Vec<u16> = "UNC\\".encode_utf16().collect();
    if let Some(share) = rest.strip_prefix(&unc[..]) {
        return [BACKSLASH, BACKSLASH]
            .iter()
            .chain(share)
            .copied()
            .collect();
    }
    match rest {
        [drive, COLON, ..] if is_ascii_letter(*drive) => rest.to_vec(),
        _ => path.to_vec(),
    }
}

/// Whether `unit` is `A`-`Z` or `a`-`z`: a drive letter.
#[cfg(any(windows, test))]
fn is_ascii_letter(unit: u16) -> bool {
    u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[cfg(windows)]
mod windows_tests;
