//! The owners of the items set aside, for the pool-user reuse rule (`Why::SetAside`). A moved
//! item keeps its owner (`set_aside/moves.rs`), and its owner has the implied `WRITE_DAC` on
//! it, so a pool user still owning an item set aside from one matter must not go to another.
//!
//! Each `<data_dir>\sandbox\set-aside\<matter>\manifest.json` is read as the set-aside code
//! reads it: the file checked first (`write::check_owned_file`), then parsed, at version 1. It
//! is read as plain JSON here, so a newer field doesn't make it unreadable. It fails closed:
//! a folder that can't be listed, or a manifest that can't be checked, read or parsed, holds
//! every pool user, and so does an item on disk whose owner wasn't recorded.
//!
//! No set-aside lock is taken: manifests are replaced whole (`write::write_atomic`), so each
//! read sees one whole version.

use std::io;
use std::path::Path;

use serde_json::Value;

use super::super::pool::assign::SetAsideOwners;
use super::super::set_aside::SET_ASIDE_DIR;
use super::super::write;
use super::SANDBOX_DIR;

/// The manifest's file name in each matter's set-aside folder.
const MANIFEST_FILE: &str = "manifest.json";
/// The manifest version this reads.
const MANIFEST_VERSION: u64 = 1;

/// The owners of every item set aside under `data_dir` and still on disk.
pub(super) fn set_aside_owners(data_dir: &Path) -> SetAsideOwners {
    let root = data_dir.join(SANDBOX_DIR).join(SET_ASIDE_DIR);
    let mut owners = SetAsideOwners::default();
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
        if !read_manifest(&path.join(MANIFEST_FILE), &mut owners) {
            return SetAsideOwners::all();
        }
    }
    owners
}

/// Adds to `owners` the owner of each item `path`'s manifest lists, finished or begun, whose
/// item is still on disk. False when the manifest can't be checked, read or parsed.
fn read_manifest(path: &Path, owners: &mut SetAsideOwners) -> bool {
    if write::check_owned_file(path).is_err() {
        return false;
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return true,
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
        if on_disk(entry) && !owners.add(entry.get("owner")) {
            return false;
        }
    }
    // A move begun may have left its item at either end; its owner holds either way.
    for intent in intents {
        if !owners.add(intent.get("owner")) {
            return false;
        }
    }
    true
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
