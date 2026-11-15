//! Unix: what a rewrite of a lawyer's file keeps of the file it replaces, copied to the temp
//! file before the rename. The permission bits (`0o777`; never setuid, setgid or sticky). On macOS
//! and Linux every extended attribute the owner may set: Finder tags
//! (`com.apple.metadata:_kMDItemUserTags`), `com.apple.FinderInfo`, the quarantine flag, Linux
//! `user.*`, and the Linux ACL (`system.posix_acl_access`); on macOS the ACL through `copyfile`.
//! Ownership is not copied: only root may give a file away, and the temp file is the writer's.
//!
//! `com.apple.quarantine` is copied like the rest: the rewritten file is the same downloaded
//! document, so Word's Protected View and Gatekeeper treat it as they treated the original.
//! Dropping the flag would launder a downloaded file through Clauseo.
//!
//! The order lets a read-only original (`0o444`) keep everything: setting an attribute needs a
//! writable file, so the mode comes after the attributes. macOS: the attributes, then the ACL,
//! then the mode. Linux: the attributes but the ACL, then the mode, then the ACL attribute
//! (setting it rewrites the group bits to the ACL's mask, as the original has them).
//!
//! Nothing here fails the write: each attribute that could not be read or set counts as one lost,
//! with its errno, and the caller logs the total once.

use std::fs::{File, Permissions};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

use super::Lost;

/// The largest attribute value copied (a resource fork can be large); a larger one counts as
/// lost. Linux caps values at 64 KiB, so only macOS meets it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_VALUE: usize = 64 << 20;

/// The Linux ACL's attribute: copied after the mode.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const ACL_ACCESS: &[u8] = b"system.posix_acl_access";

/// Copies what the file `original` has to the open temp file `temp`; what could not be copied.
/// A missing `original` (a new file, or a restore of a deleted one: checkpoints record no
/// metadata) or one that is not a plain file leaves the temp file with the defaults, and is no
/// loss.
pub(super) fn copy(original: &Path, temp: &File) -> Lost {
    let metadata = match std::fs::symlink_metadata(original) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return Lost::default(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Lost::default(),
        Err(err) => return Lost::one(err.raw_os_error()),
    };
    let mut lost = Lost::default();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    lost.add(xattrs::copy(original, temp, |name| name != ACL_ACCESS));
    #[cfg(target_os = "macos")]
    if let Err(err) = super::macos::copy_acl(original, temp) {
        lost.add(Lost::one(err.raw_os_error()));
    }
    if let Err(err) = temp.set_permissions(Permissions::from_mode(metadata.mode() & 0o777)) {
        lost.add(Lost::one(err.raw_os_error()));
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    lost.add(xattrs::copy(original, temp, |name| name == ACL_ACCESS));
    lost
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod xattrs {
    use std::fs::File;
    use std::path::Path;

    use rustix::io::Errno;

    use super::{Lost, MAX_VALUE};

    /// Copies each extended attribute of `original` (not through a symlink) that `pick` takes to
    /// `temp`, but the system's own ([`system_managed`]); what was not copied. A file system
    /// without extended attributes has none to copy.
    pub(super) fn copy(original: &Path, temp: &File, pick: impl Fn(&[u8]) -> bool) -> Lost {
        let names = match list(original) {
            Ok(names) => names,
            Err(err) if err == Errno::NOTSUP || err == Errno::OPNOTSUPP => {
                return Lost::default();
            }
            Err(err) => return Lost::one(Some(err.raw_os_error())),
        };
        let mut lost = Lost::default();
        for name in names.split(|&byte| byte == 0) {
            if name.is_empty() || system_managed(name) || !pick(name) {
                continue;
            }
            let set = value(original, name).and_then(|value| match value {
                Some(value) => {
                    let flags = rustix::fs::XattrFlags::empty();
                    rustix::fs::fsetxattr(temp, name, &value, flags)
                }
                // Removed since the listing.
                None => Ok(()),
            });
            if let Err(err) = set {
                lost.add(Lost::one(Some(err.raw_os_error())));
            }
        }
        lost
    }

    /// The NUL-separated names of `path`'s extended attributes.
    fn list(path: &Path) -> rustix::io::Result<Vec<u8>> {
        let size = rustix::fs::llistxattr_query(path)?;
        let mut names = vec![0; size];
        if size > 0 {
            let len = rustix::fs::llistxattr(path, names.as_mut_slice())?;
            names.truncate(len);
        }
        Ok(names)
    }

    /// The value of `path`'s attribute `name`; `None` once it is gone.
    fn value(path: &Path, name: &[u8]) -> rustix::io::Result<Option<Vec<u8>>> {
        let size = match rustix::fs::lgetxattr_query(path, name) {
            Ok(size) => size,
            Err(err) if absent(err) => return Ok(None),
            Err(err) => return Err(err),
        };
        if size > MAX_VALUE {
            return Err(Errno::FBIG);
        }
        let mut value = vec![0; size];
        if size > 0 {
            match rustix::fs::lgetxattr(path, name, value.as_mut_slice()) {
                Ok(len) => value.truncate(len),
                Err(err) if absent(err) => return Ok(None),
                Err(err) => return Err(err),
            }
        }
        Ok(Some(value))
    }

    /// The error for an attribute that is not there.
    fn absent(err: Errno) -> bool {
        #[cfg(target_os = "macos")]
        {
            err == Errno::NOATTR
        }
        #[cfg(target_os = "linux")]
        {
            err == Errno::NODATA
        }
    }

    /// macOS attributes only the system sets (the sandbox's `com.apple.macl`, the kernel's
    /// `com.apple.provenance`, SIP's `com.apple.rootless`, and the `com.apple.system.` names):
    /// setting them fails for any app, so they are skipped, not counted as lost.
    pub(in crate::write) fn system_managed(name: &[u8]) -> bool {
        name.starts_with(b"com.apple.system.")
            || matches!(
                name,
                b"com.apple.macl" | b"com.apple.provenance" | b"com.apple.rootless"
            )
    }
}

#[cfg(test)]
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::write) use xattrs::system_managed;
