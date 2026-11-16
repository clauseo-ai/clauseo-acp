//! Unix: what a rewrite of a lawyer's file keeps of the file it replaces, copied to the temp
//! file before the rename. The permission bits (`0o777`; never setuid, setgid or sticky). On macOS
//! and Linux every extended attribute the owner may set: Finder tags
//! (`com.apple.metadata:_kMDItemUserTags`), `com.apple.FinderInfo`, the quarantine flag, Linux
//! `user.*`, and the Linux ACL (`system.posix_acl_access`). On macOS the birth time (Finder's
//! "Created"; Linux has no settable one), and the ACL, after the rename ([`After`]). No file
//! flags (`uchg`, `schg`): they are not copied, so a temp file never has one.
//! Ownership is not copied: only root may give a file away, and the temp file is the writer's.
//!
//! `com.apple.quarantine` is copied like the rest, byte for byte: the rewritten file is the same
//! downloaded document, so Word's Protected View and Gatekeeper treat it as they treated the
//! original. Dropping the flag would launder a downloaded file through Clauseo.
//!
//! The order lets a read-only original (`0o444`) keep everything: setting an attribute needs a
//! writable file, so the mode comes after the attributes. Linux: the attributes but the ACL, then
//! the mode, then the ACL attribute (setting it rewrites the group bits to the ACL's mask, as the
//! original has them); a Linux ACL cannot keep a file from being removed (that is the folder's
//! write permission), so it goes on the temp file. macOS: the attributes, then the mode, and the
//! ACL only once the temp file is the target: a macOS ACL can deny `delete`, and on the temp file
//! that would refuse its removal after a failed rename. The cost: from the rename to the ACL, the
//! file has the folder's inherited ACL, and a crash between them leaves it so (the content is
//! the new one; only the ACL is lost).
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

/// What is copied once the temp file is the target: on macOS the original's ACL, from the
/// original, still open, to the temp file. Nothing elsewhere.
#[derive(Debug)]
pub(super) struct After {
    #[cfg(target_os = "macos")]
    acl: Option<(File, File)>,
}

impl After {
    /// Copies what waited for the rename; what could not be copied. The file is synced again,
    /// so the sync covers it.
    pub(super) fn apply(self) -> Lost {
        #[cfg(not(target_os = "macos"))]
        let Self {} = self;
        #[cfg(target_os = "macos")]
        if let Some((original, temp)) = self.acl {
            let copied = super::macos::copy_acl(&original, &temp)
                .and_then(|()| crate::fsync::sync_file(&temp));
            if let Err(err) = copied {
                return Lost::one(err.raw_os_error());
            }
        }
        Lost::default()
    }
}

/// Copies what the file `original` has to the open temp file `temp`; what could not be copied,
/// and what waits for the rename. A missing `original` (a new file, or a restore of a deleted
/// one: checkpoints record no metadata) or one that is not a plain file leaves the temp file
/// with the defaults, and is no loss.
pub(super) fn copy(original: &Path, temp: &File) -> (Lost, After) {
    let none = After {
        #[cfg(target_os = "macos")]
        acl: None,
    };
    let metadata = match std::fs::symlink_metadata(original) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return (Lost::default(), none),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return (Lost::default(), none),
        Err(err) => return (Lost::one(err.raw_os_error()), none),
    };
    let mut lost = Lost::default();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    lost.add(xattrs::copy(original, temp, |name| name != ACL_ACCESS));
    if let Err(err) = temp.set_permissions(Permissions::from_mode(metadata.mode() & 0o777)) {
        lost.add(Lost::one(err.raw_os_error()));
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    lost.add(xattrs::copy(original, temp, |name| name == ACL_ACCESS));
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt as _;
        let (secs, nanos) = (metadata.st_birthtime(), metadata.st_birthtime_nsec());
        if let Err(err) = super::macos::set_birth_time(temp, secs, nanos) {
            lost.add(Lost::one(err.raw_os_error()));
        }
    }
    #[cfg(target_os = "macos")]
    let none = {
        let files =
            super::open_read_nofollow(original).and_then(|from| Ok((from, temp.try_clone()?)));
        match files {
            Ok(files) => After { acl: Some(files) },
            Err(err) => {
                lost.add(Lost::one(err.raw_os_error()));
                none
            }
        }
    };
    (lost, none)
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
