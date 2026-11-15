//! Windows' 8.3 short names (`LONGPA~1`) spelt out in full before a picked folder is shown or
//! registered ([`long_name`]). A short name reaches the app in a path typed into the chooser, a
//! folder under a `TEMP` that Windows gave in short form (the e2e build's matter folder), or a
//! remembered last pick; the registry then keeps it and every window shows it. Each generated
//! short name holds a `~`, so a path without one is never looked up: no disk or share is
//! touched for it. A folder that does not exist yet has its longest existing ancestor spelt out
//! and the rest put back as written. Elsewhere than Windows the path comes back as it is.

#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// `path` with every 8.3 short name in it spelt out in full (`GetLongPathNameW`), on its longest
/// existing ancestor; `path` as it is when it has no `~`, when no ancestor with one can be read,
/// and on every OS but Windows.
pub(crate) fn long_name(path: &Path) -> PathBuf {
    if !may_be_short(path) {
        return path.to_path_buf();
    }
    expand(path, os_long_name).unwrap_or_else(|| path.to_path_buf())
}

/// Whether a part of `path` holds `~`, the mark of a generated 8.3 short name.
fn may_be_short(path: &Path) -> bool {
    path.components().any(|part| match part {
        Component::Normal(name) => name.as_encoded_bytes().contains(&b'~'),
        _ => false,
    })
}

/// `path` spelt out by `long_of` on its longest ancestor (itself first) that `long_of` reads,
/// with the parts below it put back as written. `None` when no ancestor that may hold a short
/// name can be read, or when `path` ends in a part with no name (`..`).
fn expand(path: &Path, long_of: impl Fn(&Path) -> Option<PathBuf>) -> Option<PathBuf> {
    let mut below: Vec<&OsStr> = Vec::new();
    let mut head = path;
    while may_be_short(head) {
        if let Some(mut long) = long_of(head) {
            long.extend(below.iter().rev());
            return Some(long);
        }
        below.push(head.file_name()?);
        head = head.parent()?;
    }
    None
}

/// `call` with a buffer it fills, as Win32 path calls take one: the length written when it fits
/// (less than the buffer), the length needed with the NUL when it does not, 0 on failure. Starts
/// at 260 units and grows to the length asked, twice at most; the units written, or `None`.
#[cfg(any(windows, test))]
fn with_room(mut call: impl FnMut(&mut [u16]) -> u32) -> Option<Vec<u16>> {
    let mut room = vec![0_u16; 260];
    for _ in 0..3 {
        let written = usize::try_from(call(&mut room)).ok()?;
        if written == 0 {
            return None;
        }
        if written < room.len() {
            room.truncate(written);
            return Some(room);
        }
        room.resize(written.saturating_add(1), 0);
    }
    None
}

#[cfg(windows)]
use windows::os_long_name;

/// Not Windows: no short names, so nothing to spell out.
#[cfg(not(windows))]
fn os_long_name(_: &Path) -> Option<PathBuf> {
    None
}
