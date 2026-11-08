//! macOS: the mount points of remote file systems, from the kernel's mount list (`getfsstat`),
//! which never opens a path.
#![expect(unsafe_code, reason = "FFI to getfsstat")]

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;

/// The file system types that live on another machine, or mount one on first touch.
const REMOTE_TYPES: [&str; 5] = ["smbfs", "nfs", "afpfs", "webdav", "autofs"];

/// Room for mounts made between the count and the read.
const SPARE: usize = 8;

/// Each mount without `MNT_LOCAL`, or of a [`REMOTE_TYPES`] type. Empty when the list can't be
/// read (logged).
pub(super) fn remote_mounts() -> Vec<PathBuf> {
    // SAFETY: a null buffer of size 0 asks only for the number of mounts.
    let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    let Ok(count) = usize::try_from(count) else {
        tracing::warn!("getfsstat failed; no mount counts as remote");
        return Vec::new();
    };
    let room = count.saturating_add(SPARE);
    // SAFETY: `statfs` is plain data, for which all zeroes is a valid value.
    let blank: libc::statfs = unsafe { std::mem::zeroed() };
    let mut list = vec![blank; room];
    let size = room.saturating_mul(size_of::<libc::statfs>());
    let size = libc::c_int::try_from(size).unwrap_or(libc::c_int::MAX);
    // SAFETY: `list` holds `room` entries and `size` is at most their size in bytes; the call
    // writes no more than `size` bytes and keeps no pointer to `list`.
    let got = unsafe { libc::getfsstat(list.as_mut_ptr(), size, libc::MNT_NOWAIT) };
    let Ok(got) = usize::try_from(got) else {
        tracing::warn!("getfsstat failed; no mount counts as remote");
        return Vec::new();
    };
    list.truncate(got);
    let local_flag = u32::try_from(libc::MNT_LOCAL).unwrap_or(0);
    list.iter()
        .filter(|mount| {
            let kind = bytes(&mount.f_fstypename);
            mount.f_flags & local_flag == 0 || REMOTE_TYPES.iter().any(|t| t.as_bytes() == kind)
        })
        .map(|mount| PathBuf::from(OsStr::from_bytes(&bytes(&mount.f_mntonname))))
        .collect()
}

/// A NUL-terminated C array's bytes, up to the NUL.
fn bytes(chars: &[libc::c_char]) -> Vec<u8> {
    chars
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| u8::from_ne_bytes(c.to_ne_bytes()))
        .collect()
}
