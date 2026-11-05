//! Free disk space, for the install's need check.

use std::io;
use std::path::Path;

/// The bytes free to this user at `path`, or at its nearest existing ancestor when `path`
/// is not made yet (a first install's data folder).
pub(crate) fn free_bytes(path: &Path) -> io::Result<u64> {
    let existing = path
        .ancestors()
        .find(|dir| dir.exists())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no part of the path exists"))?;
    free_at(existing)
}

#[cfg(unix)]
fn free_at(path: &Path) -> io::Result<u64> {
    let stat = nix::sys::statvfs::statvfs(path).map_err(io::Error::from)?;
    Ok(widen(stat.blocks_available()).saturating_mul(widen(stat.fragment_size())))
}

/// A block count or size as u64: their C types differ in width by OS.
#[cfg(unix)]
fn widen(value: impl Into<u64>) -> u64 {
    value.into()
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "GetDiskFreeSpaceExW is the OS's only free-space call"
)]
fn free_at(path: &Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt as _;

    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free: u64 = 0;
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call, and `free` is
    // a valid, writable u64 for the call's duration; the other outputs are null, which the
    // function allows.
    let result =
        unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&raw mut free), None, None) };
    result.map_err(io::Error::other)?;
    Ok(free)
}

#[cfg(test)]
mod tests;
