//! Taking back what in the admin folder shuts Administrators out (the VM reinstall finding,
//! 0049). A refresh made `staging\` SYSTEM-only (`refresh::STAGING_SDDL` before it granted
//! Administrators), and a successful refresh leaves its running image there as
//! `clauseo-sandbox-setup.exe.old` until the restart; a refresh killed midway leaves the folder
//! and its staged file. An elevated `--adopt` (a repair or reinstall) and `--remove` were then
//! denied it, and failed.
//!
//! [`remove_taking_back`] deletes an entry; one Windows refuses is taken back first
//! ([`LockDownFs::take_back`]: `SeTakeOwnershipPrivilege`, `SeRestorePrivilege` and
//! `SeBackupPrivilege` enabled for the call, every open with backup semantics and never through
//! a reparse point, Administrators made the owner and the locked DACL set, then the same below
//! it), then deleted. [`lock_down_with`] is `--adopt`'s lock-down over it: `staging\` and
//! `locks\` are always deleted (the refresh and the WSL deny task make them again), and never
//! fail the lock-down: one that can't be deleted (a WSL deny run holding its lock) stays out of
//! it, and `locks\` is always left out of the lock-down and the take-back of the folder. Pure:
//! tested on every OS; `take_back_os.rs` is the machine.

use std::io;
use std::path::Path;

use super::{Found, LOCKS, STAGING, expected_entry};

/// What the lock-down and the removal read and change.
pub(crate) trait LockDownFs {
    /// What is at `path`, never following a link.
    fn found(&mut self, path: &Path) -> io::Result<Found>;
    /// The names in the folder `dir`.
    fn entries(&mut self, dir: &Path) -> io::Result<Vec<String>>;
    /// Deletes `path`, of kind `kind`: a folder with everything in it, a link as itself.
    fn remove(&mut self, path: &Path, kind: Found) -> io::Result<()>;
    /// Gives the folder `dir` and everything in it, but `skip` (names in `dir`), the locked
    /// owner and DACL, read back; a link below is deleted.
    fn lock_tree(&mut self, dir: &Path, skip: &[String]) -> io::Result<()>;
    /// Takes `path` back for Administrators, as the module says: afterwards it, and all below
    /// it but `skip` (names in `path`), has the locked owner and DACL.
    fn take_back(&mut self, path: &Path, skip: &[String]) -> io::Result<()>;
}

/// Whether `error` is Windows refusing access (`ERROR_ACCESS_DENIED`).
fn denied(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied
}

/// What is at `path`; one Windows won't even read is taken back first.
fn found_taking_back(fs: &mut dyn LockDownFs, path: &Path) -> io::Result<Found> {
    match fs.found(path) {
        Err(error) if denied(&error) => {
            fs.take_back(path, &[])?;
            fs.found(path)
        }
        other => other,
    }
}

/// Deletes whatever is at `path` (a link as itself); when Windows refuses, takes it back
/// ([`LockDownFs::take_back`]) and deletes it again. Missing is `Ok`.
pub(crate) fn remove_taking_back(fs: &mut dyn LockDownFs, path: &Path) -> io::Result<()> {
    let kind = found_taking_back(fs, path)?;
    if kind == Found::Missing {
        return Ok(());
    }
    match fs.remove(path, kind) {
        Err(error) if denied(&error) => {
            fs.take_back(path, &[])?;
            let kind = fs.found(path)?;
            if kind == Found::Missing {
                return Ok(());
            }
            fs.remove(path, kind)
        }
        other => other,
    }
}

/// `--adopt`'s lock-down of the admin folder `dir`, whose copy is `exe`: `staging\` and
/// `locks\` deleted, taken back if need be, and only logged when even that fails (each stays
/// out of the lock-down then, as SYSTEM's alone); every other entry but the copy and its
/// signature deleted, taken back if need be; then `dir` and what is left locked, read back,
/// taken back if refused.
pub(crate) fn lock_down_with(fs: &mut dyn LockDownFs, dir: &Path, exe: &Path) -> io::Result<()> {
    if fs.found(dir)? != Found::Folder || fs.found(exe)? != Found::File {
        return Err(io::Error::other(
            "the admin folder or copy isn't a plain folder and file",
        ));
    }
    // `locks\` always: one the WSL deny task makes after the listing holds a SYSTEM-only lock
    // file, which a lock-down would hand to Administrators and Users, refusing every later run.
    let mut skip = vec![LOCKS.to_owned()];
    let mut stuck = false;
    for name in fs.entries(dir)? {
        let path = dir.join(&name);
        if name.eq_ignore_ascii_case(STAGING) || name.eq_ignore_ascii_case(LOCKS) {
            if let Err(error) = remove_taking_back(fs, &path) {
                tracing::warn!(%error, name, "--adopt: staging or locks stays");
                stuck = true;
                if !name.eq_ignore_ascii_case(LOCKS) {
                    skip.push(name);
                }
            }
            continue;
        }
        let kind = found_taking_back(fs, &path)?;
        if expected_entry(&name) && kind == Found::File {
            continue;
        }
        remove_taking_back(fs, &path)?;
    }
    match fs.lock_tree(dir, &skip) {
        Err(error) if denied(&error) && !stuck => fs.take_back(dir, &skip),
        other => other,
    }
}

#[cfg(test)]
mod tests;
