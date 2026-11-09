//! Windows: what a drive letter names, by `QueryDosDeviceW` on the letter alone (never
//! `GetDriveTypeW`, which can open a disconnected mapped drive and wait on it), and the data
//! folder's 8.3 short form. A name's kind, a reparse point's tag and a file's identity are read
//! through the no-follow open (`no_follow/windows.rs`).
#![expect(unsafe_code, reason = "FFI to QueryDosDeviceW and GetShortPathNameW")]

use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Win32::Storage::FileSystem::{GetShortPathNameW, QueryDosDeviceW};
use windows::core::{HSTRING, PCWSTR};

use super::windows_names::drive_is_local;

/// Room for a device name (`QueryDosDeviceW` writes each target NUL-terminated, then a NUL).
const DEVICE_ROOM: usize = 1024;

/// Whether drive `letter` is remote: it does not lead to a local volume
/// ([`drive_is_local`], a `subst` followed), or it can't be asked.
pub(super) fn remote_drive(letter: char) -> bool {
    !drive_is_local(letter, &device_of)
}

/// The first device target `QueryDosDeviceW` names for drive `letter`, `None` when it can't.
pub(super) fn device_of(letter: char) -> Option<String> {
    let name = HSTRING::from(format!("{letter}:"));
    let mut target = vec![0_u16; DEVICE_ROOM];
    // SAFETY: `name` is a NUL-terminated UTF-16 string, `target` a buffer of its length; the
    // call writes at most that many characters and keeps no pointer to either.
    let written = unsafe { QueryDosDeviceW(&name, Some(&mut target)) };
    if written == 0 {
        return None;
    }
    let end = target.iter().position(|&c| c == 0).unwrap_or(target.len());
    Some(String::from_utf16_lossy(
        target.get(..end).unwrap_or_default(),
    ))
}

/// `path` as a NUL-terminated UTF-16 string.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// The 8.3 short form of the local path `path` (`GetShortPathNameW`), asked only of the data
/// folder once it is proven local. `None` when it can't be read.
pub(crate) fn short_path(path: &Path) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt as _;
    let path = wide(path);
    let mut room = vec![0_u16; 512];
    for _ in 0..2 {
        // SAFETY: `path` is NUL-terminated and outlives the call; `room` is written in place,
        // at most its length.
        let written = unsafe { GetShortPathNameW(PCWSTR(path.as_ptr()), Some(&mut room)) };
        let written = usize::try_from(written).ok()?;
        if written == 0 {
            return None;
        }
        if written < room.len() {
            let short = room.get(..written)?;
            return Some(std::ffi::OsString::from_wide(short).into());
        }
        room.resize(written.saturating_add(1), 0);
    }
    None
}

/// A drive letter defined for one test (`DefineDosDeviceW`, as `subst` does) on a free letter,
/// removed when dropped.
#[cfg(test)]
pub(crate) struct TestDrive {
    /// The drive letter.
    pub(crate) letter: char,
    /// The raw device target it names (`\??\D:\folder`, `\??\UNC\server\share`).
    target: String,
}

#[cfg(test)]
impl TestDrive {
    /// A free letter (`Y` down to `M`) defined to the raw device target `target`; `None` when no
    /// letter is free or the definition fails. `DefineDosDeviceW` stacks a definition on a letter
    /// another test (another process, under nextest) took since it was seen free, without an
    /// error: so the letter's definitions are read back, and unless ours is its only one it is
    /// removed (exact match, never the other's) and the next free letter tried.
    pub(crate) fn define(target: &str) -> Option<Self> {
        use windows::Win32::Storage::FileSystem::{
            DDD_NO_BROADCAST_SYSTEM, DDD_RAW_TARGET_PATH, DefineDosDeviceW, GetLogicalDrives,
        };
        // SAFETY: no arguments; it returns a bit mask of the drives in use.
        let used = unsafe { GetLogicalDrives() };
        let free = ('M'..='Y').rev().filter(|letter| {
            let bit = u32::from(*letter) - u32::from('A');
            used & (1 << bit) == 0
        });
        for letter in free {
            let name = HSTRING::from(format!("{letter}:"));
            let raw = HSTRING::from(target);
            // SAFETY: both strings are NUL-terminated and outlive the call, which keeps neither.
            unsafe { DefineDosDeviceW(DDD_RAW_TARGET_PATH | DDD_NO_BROADCAST_SYSTEM, &name, &raw) }
                .ok()?;
            let drive = Self {
                letter,
                target: target.to_owned(),
            };
            if device_targets(letter) == [target] {
                return Some(drive);
            }
            // Another definition on the letter: ours is removed as `drive` drops.
        }
        None
    }
}

/// Every device target `QueryDosDeviceW` lists for drive `letter`, latest first (tests).
#[cfg(test)]
fn device_targets(letter: char) -> Vec<String> {
    let name = HSTRING::from(format!("{letter}:"));
    let mut room = vec![0_u16; DEVICE_ROOM];
    // SAFETY: `name` is NUL-terminated, `room` a buffer of its length; the call writes at most
    // that many characters and keeps no pointer to either.
    let written = unsafe { QueryDosDeviceW(&name, Some(&mut room)) };
    let written = usize::try_from(written).unwrap_or(0);
    room.get(..written)
        .unwrap_or_default()
        .split(|&unit| unit == 0)
        .filter(|target| !target.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

#[cfg(test)]
impl Drop for TestDrive {
    fn drop(&mut self) {
        use windows::Win32::Storage::FileSystem::{
            DDD_EXACT_MATCH_ON_REMOVE, DDD_NO_BROADCAST_SYSTEM, DDD_RAW_TARGET_PATH,
            DDD_REMOVE_DEFINITION, DefineDosDeviceW,
        };
        let name = HSTRING::from(format!("{}:", self.letter));
        let raw = HSTRING::from(self.target.as_str());
        let flags = DDD_REMOVE_DEFINITION
            | DDD_EXACT_MATCH_ON_REMOVE
            | DDD_RAW_TARGET_PATH
            | DDD_NO_BROADCAST_SYSTEM;
        // SAFETY: both strings are NUL-terminated and outlive the call, which keeps neither.
        if let Err(error) = unsafe { DefineDosDeviceW(flags, &name, &raw) } {
            eprintln!("removing the test drive {}: {error}", self.letter);
        }
    }
}
