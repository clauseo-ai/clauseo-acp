//! The permission writes as a seam (S15 PR3): [`AclWriter`], with what a write and a walk
//! report and why one fails. `NtfsAcl` (`acl.rs`) is the Windows writer; `apply` drives it,
//! and its tests use a fake.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use super::acl_plan::{Grant, OwnedRoot, Sid, verbatim};
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
///
/// In a matter's tree (`assigned` not empty, S15 `PR4h`), a child (file or folder, at any
/// depth) one of `pool` owns that isn't one of `assigned` is handed to the desktop user and
/// cleaned as [`AclWriter::reclaim`] does: that pool user keeps `WRITE_DAC` on it, so another
/// matter's agent could open it by its path whatever we deny. Its owner is read from the
/// descriptor the walk reads anyway. When that hand-back is refused for want of a right (no
/// `WRITE_OWNER`), the walk refuses as [`ApplyFailure::PoolOwned`] ([`Owned::Child`]) before
/// writing anything on it; a child the lawyer can't open stays [`ApplyFailure::ChildShut`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Strict {
    /// Whose explicit allow beats an inherited deny, besides the broad groups.
    pub watched: Vec<Sid>,
    /// Every pool user: the owners whose children are repaired.
    pub pool: Vec<Sid>,
    /// The subjects of the matter whose tree is walked (its own, state or notes folder): its
    /// pool user, and its matter SID when set; the pool owners its children may have. Empty
    /// outside a matter's tree (a sibling, the parent's files, the shared folders).
    pub assigned: Vec<Sid>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    /// A read-only path in the matter (a protected name) is a junction or symbolic link when
    /// its entry is written: an entry on a link protects neither the link nor what it leads to.
    /// `apply` turns it into [`Error::ProtectedLink`](crate::Error::ProtectedLink), naming the
    /// item. A final path other than the planned one stays [`ApplyFailure::Changed`].
    Link,
    /// A pool user owns something the plan protects, so it could take the entry off (an owner
    /// may always change the permissions), and the hand-back to the desktop user
    /// ([`AclWriter::hand_back`]) failed; what it owns, and the remedy, is `owned` (S15 `PR4h`).
    PoolOwned {
        /// What the pool user owns.
        owned: Owned,
    },
    /// The Low label couldn't be written: the lawyer may change the item's permissions but not
    /// its owner or label (no `WRITE_OWNER`). The gate refuses such a folder first
    /// (`FolderSupport::CantLabel`); this is its fallback, should the folder change in between.
    /// `apply` names the item in `Error::AclApply`'s `at`.
    CantLabel,
}

/// What a pool user owns when a launch or set-up refuses as [`ApplyFailure::PoolOwned`]: the
/// host matches on it and the app words it for the lawyer. Each is first handed to the desktop
/// user; only an item whose hand-back fails (the lawyer may not change its owner: no
/// `WRITE_OWNER`) is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owned {
    /// A root the plan denies on: a matter, state or notes folder, a registered matter's, a
    /// sibling, the parent. Remedy: the elevated clean-up (`UserSetup::reclaim`, S15 `PR4i`),
    /// for which the refusal records the item in the ledger, when the root lies in a matter
    /// root the ledger records. Otherwise (a new matter's own folder, a sibling, the parent)
    /// the clean-up refuses it: forget the matter and add it again; if the lawyer can't take
    /// it back, take ownership by hand as an administrator (its Properties, Security, Advanced,
    /// Owner). Then launch again.
    Root,
    /// A file or folder inside a matter's tree that a pool user other than the matter's own
    /// owns (one the lawyer moved in from another matter, say), whose hand-back failed. That
    /// user could open whatever is added there later. Remedy: as for [`Owned::Root`], on the
    /// item the refusal names.
    Child,
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
            ApplyFailure::Link => f.write_str("it is a junction or symbolic link"),
            ApplyFailure::PoolOwned { owned: Owned::Root } => {
                f.write_str("a sandbox user owns the folder and it couldn't be handed back")
            }
            ApplyFailure::PoolOwned {
                owned: Owned::Child,
            } => f.write_str(
                "another sandbox user owns an item in it and it couldn't be handed back",
            ),
            ApplyFailure::CantLabel => {
                f.write_str("its integrity label can't be set (no right to change its owner)")
            }
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
            | ApplyFailure::ChildShut
            | ApplyFailure::Link
            | ApplyFailure::PoolOwned { .. }
            | ApplyFailure::CantLabel) => io::Error::other(other),
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
    /// A file with more than one link is handed back too (N2). The owner is written before the
    /// cleaned DACL, and the child read again after through its handle: an owner other than the
    /// desktop user, or a pool user's allow left, refuses as [`ApplyFailure::PoolOwned`] (S15
    /// `PR4h`, replacing N13's order). A pool user's child whose handle lacks `WRITE_DAC` or
    /// `WRITE_OWNER` is refused as [`ApplyFailure::ChildShut`] (N3).
    fn reclaim(&mut self, root: &Path, pool: &[Sid]) -> Result<Walked, ApplyFailure>;
    /// [`AclWriter::reclaim`]'s hand-back for `path` itself (never what a link at it names):
    /// when one of `pool` owns it, it is handed to the desktop user and cleaned as a reclaimed
    /// child is, against its folder's entries. Whether it was. `Ok(false)` when another user
    /// owns it, or when `path` is missing, a link or not at its final path, which the write
    /// that follows refuses. An owner gets `WRITE_DAC` before any deny is read, so a pool
    /// user's protected item goes to the lawyer before its deny is written (S15 `PR4a2`). A
    /// pool user's file with more than one link is refused as [`ApplyFailure::LinkedFile`],
    /// never re-owned through one name (S15 `PR4h`).
    fn hand_back(&mut self, path: &Path, pool: &[Sid]) -> Result<bool, ApplyFailure>;
    /// The owner pass below `root`, a protected tree or a set-aside item (S15 `PR4a2`): each
    /// child's owner read through its folder's handle, never following a link; one that one of
    /// `pool` owns (with `pool` empty, none) is handed to the desktop user and cleaned as
    /// [`AclWriter::reclaim`] does; nothing else is written, a colleague's file linked in
    /// included. A junction or symbolic link one of `pool` owns is handed back as itself, never
    /// followed (its owner would keep `WRITE_DAC` on it); any other is left bare. A child the
    /// lawyer can't open is refused as [`ApplyFailure::ChildShut`], to be set aside.
    fn hand_back_tree(&mut self, root: &Path, pool: &[Sid]) -> Result<Walked, ApplyFailure>;
    /// The explicit mandatory label on `path` now, if any (ours included); `None` when the
    /// path is missing. Read before a label grant is written over it.
    fn label(&mut self, path: &Path) -> io::Result<Option<Label>>;
    /// Puts `label` back as the explicit label on `grant`'s path (a Low label grant), in place
    /// of ours or of none; another explicit label set since is left as it is.
    fn restore_label(&mut self, grant: &Grant, label: Label) -> Result<(), ApplyFailure>;
    /// Gives `root`, a folder Clauseo owns, its protected DACL (`acl_bytes::owned_dacl`): the
    /// explicit denies it has for one of `pool`, `root`'s fixed entries for the desktop user
    /// (this process's user; `OwnedRoot::fixed`), then each explicit allow it has that is exactly
    /// one of `keep`'s (the plan's and the ledger's allows there); nothing inherited, nothing
    /// else. Its owner, label and what is inside are left as they
    /// are. Through one handle opened without following a link: a link there, or a final path
    /// other than planned, is [`ApplyFailure::Changed`]; a volume's root is
    /// [`ApplyFailure::VolumeRoot`]. `AlreadyThere` when it had that DACL, protected. The
    /// default, for writers no launch drives, is `Unsupported`, so none passes unprotected.
    fn protect(
        &mut self,
        _root: &OwnedRoot,
        _pool: &[Sid],
        _keep: &[Grant],
    ) -> Result<Added, ApplyFailure> {
        Err(ApplyFailure::Io(io::ErrorKind::Unsupported))
    }
    /// The hand-back after `--reclaim` (S15 `PR4i`), and nothing else calls it: `root` (and, when
    /// `whole`, everything below it, through each folder's handle, never following a link) that the
    /// desktop user owns loses its explicit plain allows for `others` (the pool users but the
    /// matter's own), and nothing more: every other entry and the DACL's protection stay, and so
    /// does an allow `keep` lists on that path (our own entries'). An item `denies` lists as taken
    /// (its final path and file ID both match) also loses its explicit denies for the desktop user,
    /// a group of its token (enabled or deny-only), `OWNER RIGHTS`, `CREATOR OWNER` or `CREATOR
    /// GROUP`, but never one naming a SID `denies` keeps (the pool users and the pool group: our
    /// fence); a listed path whose file ID can't be read, on its own handle, is refused, so its
    /// record stays. Below a taken folder, an inherited copy of such a deny its folder no longer
    /// gives goes too. A file with more than one link is left as it is. An item left with a
    /// protected DACL and no explicit entry (only the agent's protection can have made it) is
    /// unprotected and takes its folder's inherited entries, and so is one whose inherited entries
    /// still allow one of `others` (or, when taken, deny one of those SIDs); in one write, worked
    /// out after the strip. Each item is opened with only the owner's implied `READ_CONTROL |
    /// WRITE_DAC`, and a folder listed only after its write. A child the lawyer can't open or list
    /// is refused as [`ApplyFailure::ChildShut`], naming it. The default, for writers that don't
    /// need it, is `Unsupported`.
    fn strip_taken(
        &mut self,
        _root: &Path,
        _others: &[Sid],
        _keep: &[(PathBuf, Sid)],
        _whole: bool,
        _denies: &DenyStrip<'_>,
    ) -> Result<Walked, ApplyFailure> {
        Err(ApplyFailure::Io(io::ErrorKind::Unsupported))
    }
}

/// What the hand-back after `--reclaim` may take denies off ([`AclWriter::strip_taken`]): the
/// items `--reclaim` took, each by its final path and file ID, as its result reported them and
/// the ledger keeps them; and the SIDs whose denies stay whoever owns the item, the machine
/// record's pool users and pool group.
#[derive(Debug, Clone, Copy)]
pub struct DenyStrip<'a> {
    /// The items taken.
    pub taken: &'a TakenIndex,
    /// The SIDs whose denies are kept.
    pub kept: &'a [Sid],
}

impl DenyStrip<'static> {
    /// Nothing taken, nothing kept: no deny goes anywhere.
    pub fn none() -> DenyStrip<'static> {
        static EMPTY: std::sync::OnceLock<TakenIndex> = std::sync::OnceLock::new();
        DenyStrip {
            taken: EMPTY.get_or_init(TakenIndex::default),
            kept: &[],
        }
    }
}

/// The items `--reclaim` took, keyed by their path as `in_folder` compares two paths (in the
/// verbatim form, without ASCII case): built once per hand-back, so each item the strip walks is
/// looked up in constant time, never by a scan of the list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TakenIndex {
    by_path: HashMap<Vec<u8>, Vec<(PathBuf, FileId)>>,
}

impl TakenIndex {
    /// The index of `items` (path and file ID each); one listed twice counts once.
    pub fn new(items: impl IntoIterator<Item = (PathBuf, FileId)>) -> TakenIndex {
        let mut by_path: HashMap<Vec<u8>, Vec<(PathBuf, FileId)>> = HashMap::new();
        for (path, file) in items {
            let listed = by_path.entry(path_key(&path)).or_default();
            if !listed.iter().any(|(_, f)| *f == file) {
                listed.push((path, file));
            }
        }
        TakenIndex { by_path }
    }

    /// The file IDs listed at `path`, as `in_folder` compares paths; none when it isn't listed.
    pub fn ids(&self, path: &Path) -> Vec<FileId> {
        let listed = self.by_path.get(&path_key(path));
        listed.map_or_else(Vec::new, |l| l.iter().map(|(_, f)| *f).collect())
    }

    /// Whether nothing is listed.
    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    /// Every item listed, by path then file ID.
    pub fn items(&self) -> Vec<(PathBuf, FileId)> {
        let mut all: Vec<(PathBuf, FileId)> = self.by_path.values().flatten().cloned().collect();
        all.sort_by(|a, b| (&a.0, a.1.volume, a.1.id).cmp(&(&b.0, b.1.volume, b.1.id)));
        all
    }
}

/// `path`'s key in a [`TakenIndex`]: its verbatim form's parts, ASCII lower case, joined by `\`;
/// two paths have one key exactly when `in_folder` holds each in the other.
fn path_key(path: &Path) -> Vec<u8> {
    let whole = verbatim(path);
    let mut key = Vec::new();
    for part in whole.components() {
        key.extend(part.as_os_str().as_encoded_bytes().to_ascii_lowercase());
        key.push(b'\\');
    }
    key
}
