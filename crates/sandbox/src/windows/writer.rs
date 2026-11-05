//! The permission writes as a seam (S15 PR3): [`AclWriter`], with what a write and a walk
//! report and why one fails. `NtfsAcl` (`acl.rs`) is the Windows writer; `apply` drives it,
//! and its tests use a fake.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

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

/// A protected name a walk met (`.git`, `.claude` or `.mcp.json`, at any depth, links
/// included and not followed), with its file ID: what the set-aside records at launch and
/// compares when the agent exits (S15 `PR3b` round 2, N1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Found {
    /// Its path, in the form of the walk's root joined with each name below it.
    pub path: PathBuf,
    /// Its volume's serial number.
    pub volume: u64,
    /// Its 128-bit file ID (`FileIdInfo`).
    pub id: [u8; 16],
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
    /// Children a pool user owns whose entries were repaired (see [`Strict`]), or, for
    /// [`AclWriter::reclaim`], handed back to the desktop user.
    pub repaired: u64,
}

/// What a strict walk holds a child to. An explicit allow on a child, for a broad group, one of
/// `watched` (the pool group, the other pool users) or of any type but plain or callback, is
/// checked before an inherited deny, as is a protected DACL. A child one of `pool` owns (an
/// agent made it) is repaired: those allows stripped, the protection cleared, its inherited
/// entries worked out again. A child the lawyer owns is refused. A child the lawyer can't open
/// at all is refused as [`ApplyFailure::ChildShut`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Strict {
    /// Whose explicit allow beats an inherited deny, besides the broad groups.
    pub watched: Vec<Sid>,
    /// Every pool user: the owners whose children are repaired.
    pub pool: Vec<Sid>,
}

impl Walked {
    /// Adds `other`'s counts.
    pub(crate) fn add(&mut self, other: Walked) {
        self.visited += other.visited;
        self.written += other.written;
        self.skipped += other.skipped;
        self.repaired += other.repaired;
    }
}

/// An explicit mandatory label found on a target: its level (`S-1-16-<level>`), flags and
/// mask. A Low label grant replaces the lawyer's own explicit label, so the one there before is
/// read first ([`AclWriter::label`]) and put back when ours goes ([`AclWriter::restore_label`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Label {
    /// The level's relative id: `4096` Low, `8192` Medium, `12288` High.
    pub level: u32,
    /// The entry's flags (inheritance).
    pub flags: u8,
    /// The entry's mask: no-write-up `0x1`, no-read-up `0x2`, no-execute-up `0x4`.
    pub mask: u32,
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
    /// A child in a strict walk that the lawyer can't open (a DACL an agent made shuts the
    /// lawyer out), and that couldn't be set aside.
    ChildShut,
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
            ApplyFailure::ChildShut => f.write_str("a folder or file in it can't be opened"),
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
            | ApplyFailure::NoDacl
            | ApplyFailure::ChildShut) => io::Error::other(other.to_string()),
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
    /// Whether `grant`'s explicit entry is on its path. A path that is a link now, or whose
    /// final path isn't the planned one, is an error.
    fn present(&mut self, grant: &Grant) -> io::Result<bool>;
    /// Works out each file and folder below `root` from its parent's inheritable entries and
    /// the Low label, as far as `reach`, writing what changed. Skips junctions, symbolic links
    /// and files with more than one link. With `strict`, holds each child to [`Strict`].
    fn propagate(
        &mut self,
        root: &Path,
        reach: Reach,
        strict: Option<&Strict>,
    ) -> Result<Walked, ApplyFailure>;
    /// Whether `a` and `b` are the same file (one file ID on one volume), neither followed if
    /// a link: the credential link checked against the credentials file it must be (L5).
    fn same_file(&mut self, a: &Path, b: &Path) -> io::Result<bool>;
    /// [`AclWriter::add`], refused as [`ApplyFailure::Changed`] unless the object it opened
    /// to write is the file `expected` names (one file ID, compared on that same handle): the
    /// credential link's write (L5, N8).
    fn add_expecting(&mut self, grant: &Grant, expected: &Path) -> Result<Added, ApplyFailure>;
    /// The protected names ([`Found`]) the walks since the last call met. Taking them clears
    /// them.
    fn found(&mut self) -> Vec<Found>;
    /// The child the last failed walk refused, when it was one (for the lawyer to see; never
    /// logged). Taking it clears it.
    fn refused_at(&mut self) -> Option<PathBuf>;
    /// `forget_matter`'s walk below `root`: each child one of `pool` owns is handed to the
    /// desktop user (this process's user, through the lawyer's `WRITE_OWNER`), its explicit
    /// label and the allows a strict walk would strip removed, its protection cleared; then
    /// every child's inherited entries are worked out again, as [`AclWriter::propagate`] does.
    fn reclaim(&mut self, root: &Path, pool: &[Sid]) -> Result<Walked, ApplyFailure>;
    /// The explicit mandatory label on `path` now, if any (ours included); `None` when the
    /// path is missing. Read before a label grant is written over it.
    fn label(&mut self, path: &Path) -> io::Result<Option<Label>>;
    /// Puts `label` back as the explicit label on `grant`'s path (a Low label grant), in place
    /// of ours or of none; another explicit label set since is left as it is.
    fn restore_label(&mut self, grant: &Grant, label: Label) -> Result<(), ApplyFailure>;
}
