//! `--reclaim`'s walk (S15 `PR4i`) over a seam, [`OwnerTree`], so it is tested on every OS; its
//! Windows side is `acl/take.rs`. The seam can read an object's owner and what the desktop
//! user's own token would get on it, and write its owner; its one other write takes the
//! `OWNER RIGHTS` entries off an object it takes, just before the owner, and nothing else. It has
//! no call that grants, adds an entry or writes a label, so the walk can't (K1).
//!
//! What the walk may reveal and enter is bounded by access, not by the owner's files (K1). The
//! root is resolved by handle to its final path, and so is each bound (the matter roots the
//! ledger records) and each pool user's profile; a root outside every bound, at a volume's root,
//! or holding or in a profile is refused ([`admissible`](super::admissible)). A root no pool SID
//! owns that the owner can't list (`FILE_LIST_DIRECTORY`) is refused too. A refused root counts
//! once, in [`Reclaimed::roots_refused`], and nothing is said about what is in it.
//!
//! Each object below the root is opened through its folder's handle, never following a link, as
//! the permission walk opens it. A junction, a symbolic link or another name surrogate is
//! skipped and not entered; so is a file with more than one link, whose owner belongs to every
//! name. A folder is entered only when a pool SID owns it or the owner can list it; a folder
//! neither is never entered and never counted. An object a pool SID from the machine's record
//! owns, on which the desktop user lacks a right the hand-back needs (`READ_CONTROL`,
//! `WRITE_DAC` or `WRITE_OWNER`), loses its `OWNER RIGHTS` entries (which would take its new
//! owner's implied rights away) and gets the owner as its owner, and nothing else; one whose
//! entries can't be taken off isn't taken. A file no pool SID owns that the owner can't read is
//! counted as left, never touched. One a pool user owns
//! that the desktop user can hand back is left to that hand-back.

use std::io;
use std::path::{Path, PathBuf};

use super::admissible;
use crate::windows::sids::Sid;

/// `FILE_LIST_DIRECTORY` (on a file, `FILE_READ_DATA`): listing a folder.
pub const LIST: u32 = 0x0000_0001;

/// `READ_CONTROL`: reading the owner and the DACL.
pub const READ_CONTROL: u32 = 0x0002_0000;
/// `WRITE_DAC`: the hand-back's cleaned DACL.
pub const WRITE_DAC: u32 = 0x0004_0000;
/// `WRITE_OWNER`: the hand-back's owner write.
pub const WRITE_OWNER: u32 = 0x0008_0000;
/// Every right the desktop user's hand-back needs on an object.
pub const HAND_BACK: u32 = READ_CONTROL | WRITE_DAC | WRITE_OWNER;

/// What an object is, as its own handle says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A folder, entered when the walk goes below its root.
    Folder,
    /// A file with one name.
    File,
    /// A junction, symbolic link or other name surrogate: skipped, never entered.
    Link,
    /// A file with more than one link: skipped.
    Linked,
}

/// The objects the walk reads and re-owns. The writes are [`OwnerTree::set_owner`] and, just
/// before it, [`OwnerTree::drop_owner_rights`].
pub trait OwnerTree {
    /// An object opened as itself.
    type Node;
    /// A name a folder's listing gave.
    type Name;
    /// The final path of `path`, opened by its handle without following a link at its end:
    /// what a short (8.3) name, a different case or a junction on the way resolves to.
    fn resolve(&mut self, path: &Path) -> io::Result<PathBuf>;
    /// Opens `root` itself, never what a link there names; its final path must be `root`, and
    /// it may not be a volume's root.
    fn open_root(&mut self, root: &Path) -> io::Result<Self::Node>;
    /// The names in the folder `folder`, listed through its handle.
    fn names(&mut self, folder: &Self::Node) -> io::Result<Vec<Self::Name>>;
    /// Opens `name` relative to `folder`'s handle, never following a link: `None` when the open
    /// met a reparse point, which the walk skips as a link.
    fn open(&mut self, folder: &Self::Node, name: &Self::Name) -> io::Result<Option<Self::Node>>;
    /// What `node` is.
    fn kind(&self, node: &Self::Node) -> Kind;
    /// `node`'s owner.
    fn owner(&mut self, node: &Self::Node) -> io::Result<Sid>;
    /// What the desktop user's own token gets on `node` ([`HAND_BACK`]'s rights and [`LIST`]).
    fn granted(&mut self, node: &Self::Node) -> io::Result<u32>;
    /// Takes every `OWNER RIGHTS` (S-1-3-4) entry off `node`'s DACL, allow or deny, explicit or
    /// inherited, and changes nothing else; with none, writes nothing. Called on what is taken
    /// only, just before [`OwnerTree::set_owner`].
    fn drop_owner_rights(&mut self, node: &Self::Node) -> io::Result<()>;
    /// Makes `owner` the owner of `node`, and changes nothing else.
    fn set_owner(&mut self, node: &Self::Node, owner: &Sid) -> io::Result<()>;
}

/// What a walk did. Counts only: no path is kept or logged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reclaimed {
    /// Objects looked at.
    pub visited: u64,
    /// Objects given to the owner.
    pub taken: u64,
    /// Links and files with more than one link, skipped.
    pub skipped: u64,
    /// Objects the desktop user can't open that no pool user owns: reported, never touched.
    pub left: u64,
    /// Objects that couldn't be opened, read or re-owned.
    pub failed: u64,
    /// Roots refused as the module says, each counted once.
    pub roots_refused: u64,
}

impl Reclaimed {
    /// Adds `other`'s counts.
    pub fn add(&mut self, other: Reclaimed) {
        self.visited += other.visited;
        self.taken += other.taken;
        self.skipped += other.skipped;
        self.left += other.left;
        self.failed += other.failed;
        self.roots_refused += other.roots_refused;
    }
}

/// Who may take what, and where: `pool` the machine record's pool SIDs, `owner` the user who
/// asked, `bounds` the matter roots the ledger records, `profiles` the pool users' profiles.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    /// The pool SIDs from the machine's record: the owners whose objects may be taken.
    pub pool: &'a [Sid],
    /// The user who asked (`OwnerSid`): the one owner written.
    pub owner: &'a Sid,
    /// The matter roots the ledger records: a root must lie in one.
    pub bounds: &'a [PathBuf],
    /// The pool users' profile folders: a root may neither hold nor lie in one.
    pub profiles: &'a [PathBuf],
}

/// The walk at `root` (and below it when `whole`), as the module says: what one of the pool
/// owns and the desktop user can't hand back goes to the owner. An owner that is in the pool
/// takes nothing. A missing root is nothing to do.
pub fn take_ownership<T: OwnerTree>(
    tree: &mut T,
    root: &Path,
    whole: bool,
    scope: &Scope<'_>,
) -> Reclaimed {
    let mut seen = Reclaimed::default();
    let refused = |mut seen: Reclaimed| {
        seen.roots_refused += 1;
        tracing::warn!("a clean-up's root was refused");
        seen
    };
    if scope.pool.contains(scope.owner) {
        return refused(seen);
    }
    let resolved = match tree.resolve(root) {
        Ok(resolved) => resolved,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return seen,
        Err(_) => return refused(seen),
    };
    let bounds: Vec<PathBuf> = scope
        .bounds
        .iter()
        .filter_map(|b| tree.resolve(b).ok())
        .collect();
    // A profile that can't be resolved is kept as given: refusing more is the safe side.
    let profiles: Vec<PathBuf> = scope
        .profiles
        .iter()
        .map(|p| tree.resolve(p).unwrap_or_else(|_| p.clone()))
        .collect();
    if !admissible(&resolved, &bounds, &profiles) {
        return refused(seen);
    }
    let top = match tree.open_root(&resolved) {
        Ok(top) => top,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return seen,
        Err(_) => return refused(seen),
    };
    if matches!(tree.kind(&top), Kind::Link | Kind::Linked) {
        seen.skipped += 1;
        return seen;
    }
    let (Ok(current), Ok(granted)) = (tree.owner(&top), tree.granted(&top)) else {
        return refused(seen);
    };
    if !scope.pool.contains(&current) && granted & LIST == 0 {
        return refused(seen);
    }
    let mut stack = Vec::new();
    if visit(tree, &top, scope, &mut seen) && whole {
        stack.push(top);
    }
    while let Some(folder) = stack.pop() {
        let Ok(names) = tree.names(&folder) else {
            seen.failed += 1;
            continue;
        };
        for name in names {
            match tree.open(&folder, &name) {
                Ok(Some(node)) => {
                    if visit(tree, &node, scope, &mut seen) {
                        stack.push(node);
                    }
                }
                Ok(None) => seen.skipped += 1,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => seen.failed += 1,
            }
        }
    }
    tracing::info!(
        visited = seen.visited,
        taken = seen.taken,
        skipped = seen.skipped,
        left = seen.left,
        failed = seen.failed,
        "the clean-up walked a tree"
    );
    seen
}

/// One object: re-owned when the module says so. Whether the walk goes into it: a folder a pool
/// SID owns or the owner can list. A folder neither is passed over uncounted.
fn visit<T: OwnerTree>(
    tree: &mut T,
    node: &T::Node,
    scope: &Scope<'_>,
    seen: &mut Reclaimed,
) -> bool {
    let folder = match tree.kind(node) {
        Kind::Link | Kind::Linked => {
            seen.skipped += 1;
            return false;
        }
        Kind::Folder => true,
        Kind::File => false,
    };
    let (Ok(current), Ok(granted)) = (tree.owner(node), tree.granted(node)) else {
        seen.failed += 1;
        return false;
    };
    let pooled = scope.pool.contains(&current);
    if folder && !pooled && granted & LIST == 0 {
        return false;
    }
    seen.visited += 1;
    if pooled && granted & HAND_BACK != HAND_BACK {
        let taken = tree
            .drop_owner_rights(node)
            .and_then(|()| tree.set_owner(node, scope.owner));
        match taken {
            Ok(()) => seen.taken += 1,
            Err(_) => seen.failed += 1,
        }
    } else if !pooled && granted & READ_CONTROL == 0 {
        seen.left += 1;
    }
    folder
}

#[cfg(test)]
mod tests;
