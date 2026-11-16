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
//! write permission), so it goes on the temp file. macOS: the attributes, then an owner-only mode
//! (the original's owner bits, and owner read), and the ACL and the original's mode only once the
//! temp file is the target ([`After`]): a macOS ACL can deny `delete`, and on the temp file that
//! would refuse its removal after a failed rename. The ACL is read into memory here and both
//! files closed, so a write-back of many files holds no descriptor per file until its renames.
//! From the rename to the ACL, and for good if a crash or a failure comes between them, the file
//! is its owner's alone: never more open than the original (the content is the new one; the ACL
//! and the group and other bits are what is lost).
//!
//! Nothing here fails the write: each attribute that could not be read or set counts as one lost,
//! with its errno, and the caller logs the total once.

use std::fs::{File, Permissions};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

use super::Lost;
#[cfg(target_os = "macos")]
use super::temps::FileId;

/// The largest attribute value copied (a resource fork can be large); a larger one counts as
/// lost. Linux caps values at 64 KiB, so only macOS meets it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_VALUE: usize = 64 << 20;

/// The Linux ACL's attribute: copied after the mode.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const ACL_ACCESS: &[u8] = b"system.posix_acl_access";

/// What [`copy`] did: what it lost, and on macOS what waits for the rename.
#[derive(Debug)]
pub(super) struct Copied {
    pub(super) lost: Lost,
    #[cfg(target_os = "macos")]
    pub(super) after: Option<After>,
}

/// macOS: what is set once the temp file is the target: the original's ACL (read at staging,
/// held in memory), then its mode.
#[cfg(target_os = "macos")]
#[derive(Debug)]
pub(super) struct After {
    acl: Option<super::macos::Acl>,
    mode: u32,
}

#[cfg(target_os = "macos")]
impl After {
    /// Opens `target` without following a symlink and, if it is the file `id` just renamed into
    /// place, sets the ACL and then the mode through that descriptor and syncs it. On an error the
    /// file keeps its owner-only mode.
    pub(super) fn apply(self, target: &Path, id: Option<FileId>) -> std::io::Result<()> {
        let file = super::open_read_nofollow(target)?;
        let found = super::temps::identity(&file)?;
        if !found.regular || Some(found.id) != id {
            return Err(std::io::Error::other("another file since the rename"));
        }
        if let Some(acl) = &self.acl {
            super::macos::set_acl(&file, acl)?;
        }
        file.set_permissions(Permissions::from_mode(self.mode))?;
        crate::fsync::sync_file(&file)
    }
}

/// The mode a temp file has until [`After`] sets the original's `mode`: its owner bits alone,
/// and owner read, so the file can be opened again after the rename.
#[cfg(target_os = "macos")]
pub(super) const fn owner_only(mode: u32) -> u32 {
    (mode & 0o700) | 0o400
}

/// Copies what the file `original` has to the open temp file `temp`; what could not be copied,
/// and what waits for the rename. A missing `original` (a new file, or a restore of a deleted
/// one: checkpoints record no metadata) or one that is not a plain file leaves the temp file
/// with the defaults, and is no loss.
pub(super) fn copy(original: &Path, temp: &File) -> Copied {
    let none = |lost| Copied {
        lost,
        #[cfg(target_os = "macos")]
        after: None,
    };
    let metadata = match std::fs::symlink_metadata(original) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return none(Lost::default()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return none(Lost::default()),
        Err(err) => return none(Lost::one(err.raw_os_error())),
    };
    let mode = metadata.mode() & 0o777;
    let mut lost = Lost::default();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    lost.add(xattrs::copy(original, temp, |name| name != ACL_ACCESS));
    #[cfg(target_os = "macos")]
    let temp_mode = owner_only(mode);
    #[cfg(not(target_os = "macos"))]
    let temp_mode = mode;
    if let Err(err) = temp.set_permissions(Permissions::from_mode(temp_mode)) {
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
        // Read now, the original closed again at once: nothing stays open until the rename.
        let acl =
            super::open_read_nofollow(original).and_then(|from| super::macos::read_acl(&from));
        let acl = acl.unwrap_or_else(|err| {
            lost.add(Lost::one(err.raw_os_error()));
            None
        });
        Copied {
            lost,
            after: Some(After { acl, mode }),
        }
    }
    #[cfg(not(target_os = "macos"))]
    none(lost)
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
