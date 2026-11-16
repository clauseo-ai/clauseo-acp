//! The owners of the items set aside, for the pool-user reuse rule (`Why::SetAside`). A moved
//! item keeps its owner (`set_aside/moves.rs`), and its owner has the implied `WRITE_DAC` on
//! it, so a pool user still owning an item set aside from one matter must not go to another.
//! The manifest records only the top item's owner, and an agent can rename a folder the lawyer
//! owns to `.git` and fill it, so each item's whole tree on disk is walked and every owner in it
//! counts. The walk follows no link (a link's own owner counts) and stops at [`MOST_WALKED`]
//! items in all, past which every user is held. It costs a read per item set aside, which is
//! fine: `MatterSids::assign` runs it only when a matter has no pool user yet.
//!
//! Each `<data_dir>\sandbox\set-aside\<matter>\manifest.json` is read as the set-aside code
//! reads it: the file checked first (`write::check_owned_file`), then parsed, at version 1. It
//! is read as plain JSON here, so a newer field doesn't make it unreadable. It fails closed:
//! a folder that can't be listed, a manifest that can't be checked, read or parsed, a missing
//! manifest beside a pass folder (items with no list), an item on disk whose owner wasn't
//! recorded, and an item or child whose owner can't be read all hold every pool user.
//!
//! No set-aside lock is taken: manifests are replaced whole (`write::write_atomic`), so each
//! read sees one whole version.

use std::io;
use std::path::Path;

use serde_json::Value;

use super::super::pool::assign::SetAsideOwners;
use super::super::set_aside::SET_ASIDE_DIR;
use super::super::sids::Sid;
use super::super::write;
use super::SANDBOX_DIR;

/// The manifest's file name in each matter's set-aside folder.
const MANIFEST_FILE: &str = "manifest.json";
/// The manifest version this reads.
const MANIFEST_VERSION: u64 = 1;
/// The most files, folders and links the walk of every item's tree reads, in all.
pub(super) const MOST_WALKED: usize = 100_000;

/// Reads the owner of a path without following a link.
pub(super) type OwnerOf = fn(&Path) -> io::Result<Sid>;

/// The owner of `path`, read without following a link (`acl::owner`).
#[cfg(windows)]
pub(super) fn owner_of(path: &Path) -> io::Result<Sid> {
    super::super::acl::owner(path)
}

/// Off Windows there are no owner SIDs: refused, so every user is held.
#[cfg(not(windows))]
pub(super) fn owner_of(_path: &Path) -> io::Result<Sid> {
    Err(io::ErrorKind::Unsupported.into())
}

/// The owners of every item set aside under `data_dir` and still on disk, and of everything
/// in its tree, read with `owner_of`.
pub(super) fn set_aside_owners(data_dir: &Path, owner_of: OwnerOf) -> SetAsideOwners {
    let root = data_dir.join(SANDBOX_DIR).join(SET_ASIDE_DIR);
    let mut owners = SetAsideOwners::default();
    let mut left = MOST_WALKED;
    let listing = match std::fs::read_dir(&root) {
        Ok(listing) => listing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return owners,
        Err(_unlisted) => return SetAsideOwners::all(),
    };
    for item in listing {
        let Ok(item) = item else {
            return SetAsideOwners::all();
        };
        let path = item.path();
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_not_a_folder) => continue,
            Err(_unread) => return SetAsideOwners::all(),
        }
        let mut walk = Walk {
            owner_of,
            owners: &mut owners,
            left: &mut left,
        };
        if !read_manifest(&path, &mut walk) {
            return SetAsideOwners::all();
        }
    }
    owners
}

/// The walk's state: how owners are read, what was found, and how many reads are left.
struct Walk<'a> {
    owner_of: OwnerOf,
    owners: &'a mut SetAsideOwners,
    left: &'a mut usize,
}

impl Walk<'_> {
    /// Adds the owner of everything in the tree at `root`, `root` included, following no link.
    /// False when anything in it can't be read, or the walk runs out of reads.
    fn tree(&mut self, root: &Path) -> bool {
        let mut stack = vec![root.to_owned()];
        while let Some(path) = stack.pop() {
            let Some(left) = self.left.checked_sub(1) else {
                return false;
            };
            *self.left = left;
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                return false;
            };
            let Ok(owner) = (self.owner_of)(&path) else {
                return false;
            };
            self.owners.add_owner(&owner);
            if meta.is_dir() && !meta.file_type().is_symlink() {
                let Ok(listing) = std::fs::read_dir(&path) else {
                    return false;
                };
                for child in listing {
                    let Ok(child) = child else {
                        return false;
                    };
                    stack.push(child.path());
                }
            }
        }
        true
    }
}

/// Adds the owners of each item the manifest in the matter's set-aside folder `folder` lists,
/// finished or begun: its recorded owner and every owner in its tree on disk. False when the
/// manifest can't be checked, read or parsed, is missing beside a pass folder, or a tree can't
/// be walked.
fn read_manifest(folder: &Path, walk: &mut Walk<'_>) -> bool {
    let path = folder.join(MANIFEST_FILE);
    if write::check_owned_file(&path).is_err() {
        return false;
    }
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return !has_folder(folder),
        Err(_unread) => return false,
    };
    let Ok(Value::Object(manifest)) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    if manifest.get("version").and_then(Value::as_u64) != Some(MANIFEST_VERSION) {
        return false;
    }
    let Some(Value::Array(entries)) = manifest.get("entries") else {
        return false;
    };
    let intents = match manifest.get("intents") {
        None => &[][..],
        Some(Value::Array(intents)) => intents,
        Some(_) => return false,
    };
    for entry in entries {
        if on_disk(entry) && !(walk.owners.add(entry.get("owner")) && item(entry, walk)) {
            return false;
        }
    }
    // A move begun may have left its item at either end; its owner holds either way.
    for intent in intents {
        if !walk.owners.add(intent.get("owner")) || (on_disk(intent) && !item(intent, walk)) {
            return false;
        }
    }
    true
}

/// Walks the tree of `entry`'s item at its `to`; false when `to` isn't text, so can't be
/// walked.
fn item(entry: &Value, walk: &mut Walk<'_>) -> bool {
    entry
        .get("to")
        .and_then(Value::as_str)
        .is_some_and(|to| walk.tree(Path::new(to)))
}

/// Whether `folder` holds a folder (a pass's folder, which holds items set aside), or can't be
/// listed.
fn has_folder(folder: &Path) -> bool {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return true;
    };
    listing.into_iter().any(|child| match child {
        Ok(child) => !matches!(std::fs::symlink_metadata(child.path()), Ok(m) if m.is_file()),
        Err(_unlisted) => true,
    })
}

/// Whether `entry`'s item may still be on disk: its `to` exists, or can't be checked (a path
/// that isn't text, as `set_aside/names.rs` writes one, or an error other than not found).
fn on_disk(entry: &Value) -> bool {
    let Some(to) = entry.get("to").and_then(Value::as_str) else {
        return true;
    };
    match std::fs::symlink_metadata(to) {
        Ok(_) => true,
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}
