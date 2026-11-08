//! [`LockDownFs`] on NTFS: `--adopt`'s lock-down and `--remove`'s teardown of the admin
//! folder (`take_back.rs`). A take-back runs with `SeTakeOwnershipPrivilege`,
//! `SeRestorePrivilege` and `SeBackupPrivilege` enabled for the call (`acl::privilege`, as
//! `--reclaim` enables them), so its opens, all with backup semantics and none through a reparse
//! point (`os::lock_down`, `os::found`), get `READ_CONTROL`, `WRITE_DAC`, `WRITE_OWNER` and the
//! listing whatever the DACL says. An elevated or SYSTEM token holds all three.
#![expect(
    clippy::disallowed_methods,
    reason = "deletes in the admin folder, only administrators' to change, which holds no file \
              of the lawyer's: not the data folder's write module's work"
)]

use std::io;
use std::path::Path;

use super::Found;
use super::os::{self, lock_down, lock_tree_skipping};
use super::take_back::LockDownFs;
use crate::windows::acl::privilege::Privileges;

/// What a take-back enables.
const TAKE_BACK: [&str; 3] = [
    "SeTakeOwnershipPrivilege",
    "SeRestorePrivilege",
    "SeBackupPrivilege",
];

/// The admin folder on NTFS.
pub(crate) struct NtfsLockDown;

impl LockDownFs for NtfsLockDown {
    fn found(&mut self, path: &Path) -> io::Result<Found> {
        os::found(path)
    }

    fn entries(&mut self, dir: &Path) -> io::Result<Vec<String>> {
        std::fs::read_dir(dir)?
            .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
            .collect()
    }

    fn remove(&mut self, path: &Path, kind: Found) -> io::Result<()> {
        match kind {
            Found::Folder => std::fs::remove_dir_all(path),
            Found::File => std::fs::remove_file(path),
            // A link to a folder goes with `remove_dir`, any other with `remove_file`.
            Found::Link => std::fs::remove_file(path).or_else(|_file| std::fs::remove_dir(path)),
            Found::Missing => Ok(()),
        }
    }

    fn lock_tree(&mut self, dir: &Path, skip: Option<&str>) -> io::Result<()> {
        lock_tree_skipping(dir, skip)
    }

    fn take_back(&mut self, path: &Path) -> io::Result<()> {
        let _privileges = Privileges::enable(&TAKE_BACK)?;
        match os::found(path)? {
            Found::Folder => lock_tree_skipping(path, None),
            Found::File => lock_down(path, false),
            // A link is deleted as itself, never taken back through.
            Found::Link | Found::Missing => Ok(()),
        }
    }
}
