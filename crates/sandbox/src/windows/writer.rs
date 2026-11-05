//! The permission writes as a seam (S15 PR3): [`AclWriter`], with what a write and a walk
//! report and why one fails. `NtfsAcl` (`acl.rs`) is the Windows writer; `apply` drives it,
//! and its tests use a fake.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use super::acl_plan::{Grant, Sid};
use super::ledger::RecordedFile;

/// How far a walk goes below its root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Everything below, except what [`AclWriter::propagate`] skips.
    Tree,
    /// Only the files directly in the folder
    /// ([`Inherit::FilesInFolder`](super::acl_plan::Inherit::FilesInFolder)).
    FilesOnly,
    /// Every file and folder directly in the folder, each worked out from it, and nothing
    /// deeper: a later launch's targeted walk of a folder whose names changed (S15 `PR3d`).
    Children,
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
    /// Junctions, symbolic links and files with more than one link, left bare. A file with
    /// more than one link that a pool user owns is not skipped by a strict walk or a hand-back
    /// (N2).
    pub skipped: u64,
    /// Children a pool user owns whose entries were repaired (see [`Strict`]), or, for
    /// [`AclWriter::reclaim`], handed back to the desktop user.
    pub repaired: u64,
}

/// What a strict walk holds a child to. An explicit allow on a child, for a broad group, one of
/// `watched` (the pool group, the other pool users) or of any type but plain or callback, is
/// checked before an inherited deny, as is a protected DACL. A child one of `pool` owns (an
/// agent made it) is repaired: those allows stripped, the protection cleared, its inherited
/// entries worked out again; one with more than one link too, since its DACL belongs to the file,
/// not to one name (N2). A child the lawyer owns is refused. A child the lawyer can't open at
/// all is refused as [`ApplyFailure::ChildShut`], and so is a pool user's child the walk needs
/// to write whose handle got no `WRITE_DAC` (or no `WRITE_OWNER`, for a label), so it is set
/// aside rather than left as it is (N3).
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

/// A file's identity: its volume's serial number and its 128-bit file ID (`FILE_ID_INFO`). The
/// same ID means the same file, whatever name it is opened by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileId {
    /// The volume's serial number.
    pub volume: u64,
    /// The file's ID on that volume.
    pub id: [u8; 16],
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

impl std::error::Error for ApplyFailure {}

/// The failure itself rides inside the error, so [`ApplyFailure::of`] gets it back.
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
            | ApplyFailure::ChildShut) => io::Error::other(other),
        }
    }
}

impl ApplyFailure {
    /// The failure `error` carries, when it was made from one (`Changed` stays `Changed`,
    /// S15 `PR3b` round 6); otherwise [`ApplyFailure::Io`] of its kind.
    pub fn of(error: &io::Error) -> ApplyFailure {
        error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<ApplyFailure>())
            .copied()
            .unwrap_or(ApplyFailure::Io(error.kind()))
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
    /// The ID of the file `path` names, opened as itself: a junction or symbolic link there
    /// is an error, never followed. `None` when the path is missing. A credential link's
    /// entries record the credentials file by it, so they are taken back from that file even
    /// once the link names another (S15 `PR3b` round 4, L5).
    fn file_id(&mut self, path: &Path) -> io::Result<Option<FileId>>;
    /// Removes `grant`'s explicit entry from the file `file` recorded, opened by its ID on its
    /// volume wherever its names are now (never through a path, so never through a link), and
    /// with `prior` puts that label back as [`AclWriter::restore_label`] does. Another file at
    /// the recorded path is left alone. `Ok` when no file has that ID any more: it took its
    /// entries with it (S15 `PR3b` round 5).
    fn remove_recorded(
        &mut self,
        file: &RecordedFile,
        grant: &Grant,
        prior: Option<Label>,
    ) -> Result<(), ApplyFailure>;
    /// Whether `grant`'s explicit entry is on the file `file` recorded, opened by its ID as
    /// [`AclWriter::remove_recorded`] opens it; `false` when no file has that ID any more.
    fn present_recorded(&mut self, file: &RecordedFile, grant: &Grant) -> io::Result<bool>;
    /// [`AclWriter::add`], refused as [`ApplyFailure::Changed`] unless the object it opened
    /// to write is the file `expected` names (one file ID, compared on that same handle): the
    /// credential link's write (L5, N8).
    fn add_expecting(&mut self, grant: &Grant, expected: &Path) -> Result<Added, ApplyFailure>;
    /// The child the last failed walk refused, when it was one (for the lawyer to see; never
    /// logged). Taking it clears it.
    fn refused_at(&mut self) -> Option<PathBuf>;
    /// `forget_matter`'s walk below `root`: each child one of `pool` owns is handed to the
    /// desktop user (this process's user, through the lawyer's `WRITE_OWNER`), its explicit
    /// label and the allows a strict walk would strip removed, its protection cleared; then
    /// every child's inherited entries are worked out again, as [`AclWriter::propagate`] does.
    /// A file with more than one link is handed back too (N2). The cleaned DACL is written
    /// before the owner, so a failure between them leaves a file a pool user still owns, which
    /// the next hand-back takes again (N13). A pool user's child whose handle lacks
    /// `WRITE_DAC` or `WRITE_OWNER` is refused as [`ApplyFailure::ChildShut`] (N3).
    fn reclaim(&mut self, root: &Path, pool: &[Sid]) -> Result<Walked, ApplyFailure>;
    /// The explicit mandatory label on `path` now, if any (ours included); `None` when the
    /// path is missing. Read before a label grant is written over it.
    fn label(&mut self, path: &Path) -> io::Result<Option<Label>>;
    /// Puts `label` back as the explicit label on `grant`'s path (a Low label grant), in place
    /// of ours or of none; another explicit label set since is left as it is.
    fn restore_label(&mut self, grant: &Grant, label: Label) -> Result<(), ApplyFailure>;
}
