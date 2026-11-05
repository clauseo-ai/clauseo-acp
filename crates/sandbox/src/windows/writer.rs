//! The permission writes as a seam (S15 PR3): [`AclWriter`], with what a write and a walk
//! report and why one fails. `NtfsAcl` (`acl.rs`) is the Windows writer; `apply` drives it,
//! and its tests use a fake.

use std::fmt;
use std::io;
use std::path::Path;

use super::acl_plan::{Grant, Sid};

/// How far a walk goes below its root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Everything below, except what [`AclWriter::propagate`] skips.
    Tree,
    /// Only the files directly in the folder
    /// ([`Inherit::FilesInFolder`](super::acl_plan::Inherit::FilesInFolder)).
    FilesOnly,
}

/// What a write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    /// The entry was written.
    Written,
    /// The entry was there already.
    AlreadyThere,
}

/// What a walk saw.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Walked {
    /// Files and folders whose entries were worked out.
    pub visited: u64,
    /// Of those, the ones whose security descriptor changed.
    pub written: u64,
    /// Junctions, symbolic links and files with more than one link, left bare.
    pub skipped: u64,
}

/// Why an apply, a removal or a walk failed. Names no path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyFailure {
    /// A child in a tree that carries a deny has a protected DACL: no inherited deny reaches it.
    ProtectedChild,
    /// A child in a tree that carries a deny has an explicit allow for a broad group or another
    /// pool subject, which is checked before the inherited deny.
    BroadAllowChild,
    /// The target isn't what was planned: it is a junction or symbolic link now, or its final
    /// path differs from the planned one.
    Changed,
    /// The target is a file with more than one link, and the entry would reach the other names.
    LinkedFile,
    /// A deny for the whole tree on a volume's root.
    VolumeRoot,
    /// The target has no DACL (everyone may do anything); one entry would shut out the rest.
    NoDacl,
    /// A file-system call failed, of this kind.
    Io(io::ErrorKind),
    /// The ledger couldn't be read or written, of this kind.
    Ledger(io::ErrorKind),
}

impl fmt::Display for ApplyFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApplyFailure::ProtectedChild => f.write_str("a folder or file in it is protected"),
            ApplyFailure::BroadAllowChild => {
                f.write_str("a folder or file in it is open to every user by its own entry")
            }
            ApplyFailure::Changed => f.write_str("it changed since it was checked"),
            ApplyFailure::LinkedFile => f.write_str("it is a file with another name elsewhere"),
            ApplyFailure::VolumeRoot => f.write_str("it is a drive's root"),
            ApplyFailure::NoDacl => f.write_str("it has no permission list"),
            ApplyFailure::Io(kind) => write!(f, "a file-system call failed ({kind})"),
            ApplyFailure::Ledger(kind) => write!(f, "the permission ledger failed ({kind})"),
        }
    }
}

impl From<ApplyFailure> for io::Error {
    fn from(failure: ApplyFailure) -> io::Error {
        match failure {
            ApplyFailure::Io(kind) | ApplyFailure::Ledger(kind) => io::Error::from(kind),
            other @ (ApplyFailure::ProtectedChild
            | ApplyFailure::BroadAllowChild
            | ApplyFailure::Changed
            | ApplyFailure::LinkedFile
            | ApplyFailure::VolumeRoot
            | ApplyFailure::NoDacl) => io::Error::other(other.to_string()),
        }
    }
}

/// The permission writes, as a seam: `acl.rs`'s `NtfsAcl` on Windows, a fake in the tests.
pub trait AclWriter {
    /// Writes `grant`'s entry on its path, explicitly; a name that isn't the planned target
    /// refuses.
    fn add(&mut self, grant: &Grant) -> Result<Added, ApplyFailure>;
    /// Removes `grant`'s explicit entry; one already gone, or a missing path, is `Ok`.
    fn remove(&mut self, grant: &Grant) -> Result<(), ApplyFailure>;
    /// Whether `grant`'s explicit entry is on its path.
    fn present(&mut self, grant: &Grant) -> io::Result<bool>;
    /// Works out each file and folder below `root` from its parent's inheritable entries and
    /// the Low label, as far as `reach`, writing what changed. Skips junctions, symbolic links
    /// and files with more than one link. With `strict`, refuses a protected child or one with
    /// an explicit allow for a broad group or one of `strict`'s SIDs.
    fn propagate(
        &mut self,
        root: &Path,
        reach: Reach,
        strict: Option<&[Sid]>,
    ) -> Result<Walked, ApplyFailure>;
}
