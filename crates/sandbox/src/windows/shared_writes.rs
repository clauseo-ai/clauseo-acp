//! Places a sandbox token can write at Low integrity, and the denies that close them.
//!
//! The VM spike (private `measurements/s15-spikes-traverse.md`, (c)) probed 62,145 folders and
//! found exactly two it could write: `C:\ProgramData\Microsoft\DeviceSync`, which every pool user
//! shares, and each pool user's own `AppData\LocalLow`. A folder a lawyer makes directly under
//! `C:\` is already refused by Low integrity, so it isn't listed here.
//!
//! The group's deny closes each one. The spike measured `icacls` `(W,DC)`; the code writes
//! `0xd0156`, a superset that adds `DELETE`, `WRITE_DAC` and `WRITE_OWNER`. Tools running with
//! the full mask are not measured yet: PR4's VM test must confirm them.
//!
//! `LocalLow` is denied because it is the one Low-writable place outside the session-end repair:
//! a file an agent makes there with its own DACL would outlive the session.
//!
//! The deny applies even though allows to the group don't: deny entries are matched against the
//! token's normal groups, which the pool group is one of. The owner always has `WRITE_DAC`. The
//! files a pool user makes in its `LocalLow` are that user's own, so each `LocalLow` also gets
//! an `OWNER RIGHTS` deny ([`OWNER_RIGHTS_SID`]) that replaces the owner's implied rights: the
//! owner loses `WRITE_DAC` and `WRITE_OWNER` on what is below. The deny is inherit-only
//! ([`Inherit::Below`]): `LocalLow` itself is SYSTEM's, and keeps SYSTEM's rights there.
//! `DeviceSync` is SYSTEM's too, and Windows writes to it, so it gets no `OWNER RIGHTS` deny.
//!
//! Each grant is written, then walked down (`propagate`) with the pool group and every pool user
//! watched, and the pool users as the owners to repair. So what's already there inherits the
//! deny: a child a pool user owns has its explicit allows stripped and its entries worked out
//! again, as the launch's own walk does; a child the lawyer owns, or a protected one the walk can't
//! repair, fails its target. Files with more than one link are skipped and counted, as the walk
//! skips them.
//!
//! Each target is independent. An add failure rolls back that target's own grants; a walk failure
//! keeps them (the deny still blocks new files and covers what the walk reached) and is listed as
//! a failure. A `LocalLow` doesn't exist until its pool user's first logon, and Windows makes it
//! SYSTEM's, so PR4 logs each pool user on once, with its profile loaded, before it applies these.
//! A missing `LocalLow` is a failure; a missing `DeviceSync` has nothing to close and is skipped.
//!
//! Set-up (PR4) applies these with admin rights after it creates each pool user's profile and
//! logs the user on, and treats any failed target as a set-up failure, keeping what was written.
//! Uninstall removes them in this order: the `LocalLow` denies, then each profile, then the
//! account, then `DeviceSync`'s deny. A `LocalLow` deny stays up until its profile is gone.
//!
//! The planning and the apply take an [`AclWriter`], so they build and test on every OS; the
//! real-NTFS test is Windows-only (`tests.rs`).

use std::io;
use std::path::{Component, Path, PathBuf};

use super::acl_plan::{Grant, Inherit, Rights, Sid};
use super::admit::PathKind;
use super::sids::NotASid;
use super::writer::{AclWriter, Added, ApplyFailure, Reach, Strict};

#[cfg(test)]
mod tests;

/// The `OWNER RIGHTS` well-known SID. An entry for it replaces the owner's implied rights, which
/// always include `WRITE_DAC`.
pub const OWNER_RIGHTS_SID: &str = "S-1-3-4";

/// What [`apply_shared_write_denies`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharedWrites {
    /// The grants written, in order. An entry that was there already is not listed, so a
    /// rollback never removes a deny an earlier set-up wrote.
    pub added: Vec<Grant>,
    /// The targets that don't exist and have nothing to close: a missing `DeviceSync`.
    pub skipped: Vec<PathBuf>,
    /// The targets that failed, each with why. An add failure rolls back the target's own
    /// grants; a walk failure keeps them. The other targets were applied anyway.
    pub failed: Vec<(PathBuf, ApplyFailure)>,
    /// How many files and folders the walks skipped: junctions, symbolic links and files with
    /// more than one link.
    pub walk_skipped: u64,
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

/// The grants for `targets`, in order: each target gets `group`'s `DenyWrite`, inherited to the
/// tree; a `LocalLow` target also gets an [`OWNER_RIGHTS_SID`] `DenyWrite` right after its
/// group's, inherit-only ([`Inherit::Below`]). Refuses only if [`OWNER_RIGHTS_SID`] doesn't parse,
/// which it does.
pub fn plan_shared_write_denies(group: &Sid, targets: &[PathBuf]) -> Result<Vec<Grant>, NotASid> {
    let owner_rights = Sid::parse(OWNER_RIGHTS_SID)?;
    let mut grants = Vec::new();
    for path in targets {
        grants.push(deny(path, group.clone(), Inherit::Tree));
        if is_local_low(path) {
            grants.push(deny(path, owner_rights.clone(), Inherit::Below));
        }
    }
    Ok(grants)
}

/// One `DenyWrite` for `sid` on `path`, inherited as `inherit` says.
fn deny(path: &Path, sid: Sid, inherit: Inherit) -> Grant {
    Grant {
        path: path.to_owned(),
        sid,
        rights: Rights::DenyWrite,
        inherit,
        kind: PathKind::SharedWrite,
    }
}

/// Whether `path` ends in `AppData\LocalLow`, without ASCII case.
fn is_local_low(path: &Path) -> bool {
    let mut parts = path.components().rev().filter_map(|part| match part {
        Component::Normal(name) => Some(name),
        Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir => {
            None
        }
    });
    parts
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("LocalLow"))
        && parts
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("AppData"))
}

/// Writes the grants, target by target (consecutive grants on one path), and walks each down.
///
/// A target that doesn't exist is skipped, except a `LocalLow`, which fails with
/// `Io(NotFound)`. A target whose add fails has its own grants rolled back and is listed in
/// [`SharedWrites::failed`]. A target whose walk fails keeps its grants (the deny still blocks
/// new files, and covers what the walk reached) and is listed there too. The other targets
/// carry on. Walks are strict ([`strict_for`]): a child a pool user owns is repaired, not refused.
pub fn apply_shared_write_denies(
    writer: &mut impl AclWriter,
    grants: &[Grant],
    pool_users: &[Sid],
) -> SharedWrites {
    let strict = strict_for(grants, pool_users);
    let mut done = SharedWrites::default();
    for target in grants.chunk_by(|a, b| a.path() == b.path()) {
        let Some(first) = target.first() else {
            continue;
        };
        let path = first.path();
        match exists(path) {
            Ok(true) => {}
            Ok(false) if is_local_low(path) => {
                done.failed
                    .push((path.to_owned(), ApplyFailure::Io(io::ErrorKind::NotFound)));
                continue;
            }
            Ok(false) => {
                done.skipped.push(path.to_owned());
                continue;
            }
            Err(failure) => {
                done.failed.push((path.to_owned(), failure));
                continue;
            }
        }
        let mut written = Vec::new();
        let mut walk_skipped = 0;
        match write_target(writer, target, &strict, &mut written, &mut walk_skipped) {
            Ok(walk_failure) => {
                done.added.extend(written);
                done.walk_skipped += walk_skipped;
                if let Some(failure) = walk_failure {
                    done.failed.push((path.to_owned(), failure));
                }
            }
            Err(failure) => {
                rollback(writer, &written);
                done.failed.push((path.to_owned(), failure));
            }
        }
    }
    done
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

/// The walk's strictness: the grants' subjects (the pool group; `OWNER RIGHTS` is left out) and
/// every pool user are watched, so an explicit allow for either beats nothing; the pool users are
/// the owners whose children are repaired.
fn strict_for(grants: &[Grant], pool_users: &[Sid]) -> Strict {
    let mut watched: Vec<Sid> = Vec::new();
    for sid in grants
        .iter()
        .map(Grant::sid)
        .filter(|sid| sid.as_str() != OWNER_RIGHTS_SID)
        .chain(pool_users)
    {
        if !watched.contains(sid) {
            watched.push(sid.clone());
        }
    }
    let mut pool: Vec<Sid> = Vec::new();
    for sid in pool_users {
        if !pool.contains(sid) {
            pool.push(sid.clone());
        }
    }
    Strict {
        watched,
        pool,
        assigned: Vec::new(),
    }
}

/// Writes one target's grants in order: each entry goes in, then the walk carries it down. The
/// written grants are listed in `written` as they go. An add failure is the `Err`, and stops the
/// target. A walk failure is the `Ok(Some(..))`: the grants stay, and the rest still go in.
fn write_target(
    writer: &mut impl AclWriter,
    target: &[Grant],
    strict: &Strict,
    written: &mut Vec<Grant>,
    walk_skipped: &mut u64,
) -> Result<Option<ApplyFailure>, ApplyFailure> {
    let mut walk_failure = None;
    for grant in target {
        match writer.add(grant)? {
            Added::Written => written.push(grant.clone()),
            Added::AlreadyThere => {}
        }
        match writer.propagate(grant.path(), Reach::Tree, Some(strict)) {
            Ok(walked) => *walk_skipped += walked.skipped,
            Err(failure) => {
                walk_failure.get_or_insert(failure);
            }
        }
    }
    Ok(walk_failure)
}

/// Whether `path` is there, without following a link. Any failure but "not found" is an error.
fn exists(path: &Path) -> Result<bool, ApplyFailure> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ApplyFailure::Io(error.kind())),
    }
}

/// Removes `grant`'s entry on its folder, then walks it down so the children lose the inherited
/// deny.
fn unwrite_one(writer: &mut impl AclWriter, grant: &Grant) -> Result<(), ApplyFailure> {
    writer.remove(grant)?;
    writer.propagate(grant.path(), Reach::Tree, None)?;
    Ok(())
}

/// Undoes one target's `added` grants, last first. A failure here is not reported: the caller
/// reports the failure that stopped the target.
fn rollback(writer: &mut impl AclWriter, added: &[Grant]) {
    for grant in added.iter().rev() {
        let _unreported: Result<(), ApplyFailure> = unwrite_one(writer, grant);
    }
}
