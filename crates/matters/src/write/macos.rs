//! macOS: a file's ACL copied to another through `fcopyfile(3)` with `COPYFILE_ACL`, for
//! [`super::keep`]. macOS ACLs are not extended attributes `listxattr` shows, and libSystem's
//! `copyfile` family is the one way to copy them.
#![expect(unsafe_code, reason = "FFI to fcopyfile")]

use std::fs::File;
use std::io;
use std::os::fd::AsRawFd as _;
use std::path::Path;

/// Copies the ACL of the file `from` (opened without following a symlink) to the open file `to`;
/// with none on `from`, `to` keeps its own. `copyfile` answers 0 even when it could not apply the
/// ACL to `to`, so that case is not seen and not counted as lost. It also writes a fresh
/// quarantine stamp on `to` when `from` has one; [`super::keep`] copies the flag after it.
pub(super) fn copy_acl(from: &Path, to: &File) -> io::Result<()> {
    let from = super::open_read_nofollow(from)?;
    // SAFETY: both descriptors are open for the whole call (the files outlive it); a null state
    // is allowed and the call keeps nothing.
    let done = unsafe {
        libc::fcopyfile(
            from.as_raw_fd(),
            to.as_raw_fd(),
            std::ptr::null_mut(),
            libc::COPYFILE_ACL,
        )
    };
    if done == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
