//! Shared writable places (S15 PR3a): a deny of writes for `ClauseoSandboxUsers` on the two
//! folders a Low-integrity sandbox token can write that every pool user shares.
//!
//! The VM spike (private `measurements/s15-spikes-traverse.md`, (c)) probed 62,145 folders and
//! found exactly two: `C:\ProgramData\Microsoft\DeviceSync` and each pool user's own
//! `AppData\LocalLow`. One group deny-write on each closed both, with no tool regressions. A
//! folder a lawyer makes directly under `C:\` is already refused by Low integrity, so it isn't
//! listed here.
//!
//! The deny applies even though allows to the group don't: deny entries are matched against the
//! token's normal groups, which the pool group is one of. Each grant is walked down after it is
//! written, so what's already there inherits the deny; files with more than one link are skipped,
//! as the walk does.
//!
//! Set-up (PR4) applies these with admin rights after it creates each pool user's profile, and
//! uninstall removes them: profile, then account, then the entries (fix 5's order).
//!
//! The planning and the apply take an [`AclWriter`], so they build and test on every OS; the
//! real-NTFS test is Windows-only (`tests.rs`).

use std::io;
use std::path::{Path, PathBuf};

use super::acl_plan::{Grant, Inherit, Rights, Sid};
use super::admit::PathKind;
use super::writer::{AclWriter, Added, ApplyFailure, Reach};

#[cfg(test)]
mod tests;

/// What [`apply_shared_write_denies`] did: the grants it wrote, and the targets it skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharedWrites {
    /// The grants this call wrote, in order. An entry that was there already is not listed, so
    /// a rollback never removes a deny an earlier set-up wrote.
    pub added: Vec<Grant>,
    /// The targets that don't exist yet, and so got no deny.
    pub skipped: Vec<PathBuf>,
}

/// The shared places, in order: `<program_data>\Microsoft\DeviceSync`, then each pool profile's
/// `AppData\LocalLow`.
pub fn shared_write_targets(program_data: &Path, pool_profiles: &[PathBuf]) -> Vec<PathBuf> {
    let device_sync = program_data.join("Microsoft").join("DeviceSync");
    let local_lows = pool_profiles
        .iter()
        .map(|profile| profile.join("AppData").join("LocalLow"));
    std::iter::once(device_sync).chain(local_lows).collect()
}

/// One grant per target: `group` denied writes (`Rights::DenyWrite`) on the target and,
/// inherited, everything under it.
pub fn plan_shared_write_denies(group: &Sid, targets: &[PathBuf]) -> Vec<Grant> {
    targets
        .iter()
        .map(|path| Grant {
            path: path.clone(),
            sid: group.clone(),
            rights: Rights::DenyWrite,
            inherit: Inherit::Tree,
            kind: PathKind::SharedWrite,
        })
        .collect()
}

/// Writes each grant, in order, and walks it down to what's there. A target that doesn't exist
/// is skipped and listed in [`SharedWrites::skipped`]. Any other failure removes the grants this
/// call already wrote, last first, and returns the failure.
pub fn apply_shared_write_denies(
    writer: &mut impl AclWriter,
    grants: &[Grant],
) -> Result<SharedWrites, ApplyFailure> {
    let mut done = SharedWrites::default();
    for grant in grants {
        match exists(grant.path()) {
            Ok(true) => {}
            Ok(false) => {
                done.skipped.push(grant.path().to_owned());
                continue;
            }
            Err(failure) => return Err(rollback(writer, &done.added, failure)),
        }
        if let Err(failure) = write_one(writer, grant, &mut done.added) {
            return Err(rollback(writer, &done.added, failure));
        }
    }
    Ok(done)
}

/// Removes each grant whose target exists, in order, and walks it down so what's there loses
/// the inherited deny. Every grant is tried; the first failure is returned.
pub fn remove_shared_write_denies(
    writer: &mut impl AclWriter,
    grants: &[Grant],
) -> Result<(), ApplyFailure> {
    let mut first_failure = None;
    for grant in grants {
        let result = match exists(grant.path()) {
            Ok(true) => unwrite_one(writer, grant),
            Ok(false) => Ok(()),
            Err(failure) => Err(failure),
        };
        if let Err(failure) = result {
            first_failure.get_or_insert(failure);
        }
    }
    first_failure.map_or(Ok(()), Err)
}

/// Whether `path` is there, without following a link. Any failure but "not found" is an error.
fn exists(path: &Path) -> Result<bool, ApplyFailure> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ApplyFailure::Io(error.kind())),
    }
}

/// Writes `grant`'s entry on its folder, then walks it down (the walk is what gives the children
/// the inherited deny). A grant written here goes into `added` before the walk, so a failed walk
/// rolls it back too; an entry that was there already doesn't.
fn write_one(
    writer: &mut impl AclWriter,
    grant: &Grant,
    added: &mut Vec<Grant>,
) -> Result<(), ApplyFailure> {
    match writer.add(grant)? {
        Added::Written => added.push(grant.clone()),
        Added::AlreadyThere => {}
    }
    writer.propagate(grant.path(), Reach::Tree, None)?;
    Ok(())
}

/// Removes `grant`'s entry on its folder, then walks it down so the children lose the inherited
/// deny.
fn unwrite_one(writer: &mut impl AclWriter, grant: &Grant) -> Result<(), ApplyFailure> {
    writer.remove(grant)?;
    writer.propagate(grant.path(), Reach::Tree, None)?;
    Ok(())
}

/// Undoes `added`, last first, and gives back `failure`, the one that stopped the apply. An undo
/// that fails here is not reported: the caller hears the first failure.
fn rollback(writer: &mut impl AclWriter, added: &[Grant], failure: ApplyFailure) -> ApplyFailure {
    for grant in added.iter().rev() {
        let _unreported: Result<(), ApplyFailure> = unwrite_one(writer, grant);
    }
    failure
}
