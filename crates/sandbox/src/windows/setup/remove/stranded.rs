//! `--remove`'s side of `stranded.rs`: the pool accounts the record doesn't list, proven
//! Clauseo's (in `ClauseoSandboxUsers` and marked), removed; the rest named and left. Never
//! [`SetupExit::RemovedPartly`]: a stranded account has no record to finish from, and the next
//! set-up or removal finds it again by the same proof.
//!
//! - [`widen`]: a record whose `OwnerSid`, `GroupSid` and `DataDir` read, but whose pool lost
//!   users (blanked, cut short, or malformed and read as empty): each proven account the group
//!   named by `GroupSid` holds is written back into the record, with `Removing`, and then goes
//!   the way every recorded user goes (`remove.rs` step 4), its profile and rights included.
//! - [`sweep`]: no record, or one that can't be trusted ([`MachineError::RecordCorrupt`]): the
//!   group is found by name (only an administrator can make a local group), each proven member
//!   is stopped and removed with its profile, then the group with its Remote Desktop deny when
//!   it is empty; a corrupt record's set-up values go last. Nothing is handed to anyone: the
//!   SIDs die with their accounts.

use super::super::machine::MachineError;
use super::super::outcome::{SetupExit, cause_of};
use super::super::stranded::{log_doubtful, sort};
use super::{Removal, RemoveFailure, RemoveReport, RemoveStep, Removed};
use crate::windows::pool::{MachineRecord, POOL_GROUP, PoolUser};
use crate::windows::sids::Sid;

/// `SYSTEM`: who strays and profiles are checked against when no one asked and no record
/// names an owner; never a pool user's SID.
const SYSTEM: &str = "S-1-5-18";

/// What the record read gave: the record, or none, and whether none because it can't be
/// trusted.
pub(super) struct Found {
    /// The record, when there is one to trust.
    pub(super) record: Option<MachineRecord>,
    /// The record is there but [`MachineError::RecordCorrupt`].
    pub(super) corrupt: bool,
}

/// The record, or a failure with nothing touched; a corrupt one is none, flagged.
pub(super) fn read(machine: &mut impl Removal) -> Result<Found, RemoveFailure> {
    match machine.read_record() {
        Ok(record) => Ok(Found {
            record,
            corrupt: false,
        }),
        Err(MachineError::RecordCorrupt(why)) => {
            tracing::warn!(
                %why,
                "the machine's record can't be trusted; only the sandbox users that can be \
                 proven Clauseo's are removed"
            );
            Ok(Found {
                record: None,
                corrupt: true,
            })
        }
        Err(error) => Err(RemoveFailure {
            step: RemoveStep::ReadRecord,
            cause: cause_of(&error),
            exit: SetupExit::Failed,
        }),
    }
}

/// `record` with every proven account it doesn't list added to its pool, written with
/// `Removing` set when any was, and the doubtful accounts' names.
pub(super) fn widen(
    machine: &mut impl Removal,
    record: MachineRecord,
) -> Result<(MachineRecord, Vec<String>), RemoveFailure> {
    let accounts = machine.pool_accounts().map_err(|error| RemoveFailure {
        step: RemoveStep::ReadRecord,
        cause: cause_of(&error),
        exit: SetupExit::Failed,
    })?;
    let mut skip = record.pool.clone();
    skip.push(record.owner.clone());
    let mut sorted = sort(&accounts.seen, &skip);
    if accounts.group.as_ref() != Some(&record.group) {
        sorted.proven.clear();
    }
    log_doubtful(&sorted.doubtful, accounts.foreign);
    let left = sorted.doubtful.iter().map(ToString::to_string).collect();
    if sorted.proven.is_empty() {
        return Ok((record, left));
    }
    for user in &sorted.proven {
        tracing::warn!(account = %user.name, "a sandbox user the record lost is removed too");
    }
    let mut widened = record;
    widened
        .pool
        .extend(sorted.proven.into_iter().map(|user| user.sid));
    widened.removing = true;
    machine
        .write_record(&widened)
        .map_err(|error| RemoveFailure {
            step: RemoveStep::Flag,
            cause: cause_of(&error),
            exit: SetupExit::RemovedPartly,
        })?;
    Ok((widened, left))
}

/// With no record to trust: each proven account and its profile, then the group when empty,
/// then a corrupt record's values. [`Removed::NothingSetUp`] when there was nothing (no group,
/// no record); else [`Removed::All`], with what was left named in [`RemoveReport::left`].
pub(super) fn sweep(machine: &mut impl Removal, corrupt: bool) -> Result<Removed, RemoveFailure> {
    let accounts = machine.pool_accounts().map_err(|error| RemoveFailure {
        step: RemoveStep::ReadRecord,
        cause: cause_of(&error),
        exit: SetupExit::Failed,
    })?;
    let owner = match machine.invoker() {
        Some(invoker) => invoker.clone(),
        None => Sid::parse(SYSTEM).map_err(|error| RemoveFailure {
            step: RemoveStep::Owner,
            cause: error.to_string(),
            exit: SetupExit::Failed,
        })?,
    };
    let sorted = sort(&accounts.seen, std::slice::from_ref(&owner));
    log_doubtful(&sorted.doubtful, accounts.foreign);
    let mut report = RemoveReport {
        left: sorted.doubtful.iter().map(ToString::to_string).collect(),
        ..RemoveReport::default()
    };
    if let Some(group) = &accounts.group {
        let pool: Vec<Sid> = sorted.proven.iter().map(|user| user.sid.clone()).collect();
        if !pool.is_empty() {
            match machine.stop_strays(&pool, &owner) {
                Ok(0) => {}
                Ok(left) => {
                    tracing::warn!(left, "processes of the sandbox users wouldn't stop");
                }
                Err(error) => {
                    tracing::warn!(%error, "the sandbox users' processes weren't stopped");
                }
            }
        }
        for user in &sorted.proven {
            tracing::warn!(account = %user.name, "a sandbox user no record lists is removed");
            if let Err(cause) = remove_user(machine, user, &owner, &mut report) {
                tracing::warn!(
                    account = %user.name,
                    %cause,
                    "a sandbox user couldn't be removed; the next set-up or removal finds it again"
                );
                report.left.push(format!("{}: {cause}", user.name));
            }
        }
        report.group = remove_group(machine, group, &mut report.left);
    } else if !corrupt {
        return Ok(Removed::NothingSetUp);
    }
    if corrupt && let Err(error) = machine.delete_record() {
        tracing::warn!(%error, "the record that can't be trusted couldn't be deleted");
        report
            .left
            .push(format!("the machine's record: {}", cause_of(&error)));
    }
    Ok(Removed::All(report))
}

/// One proven account: its profile, then the account. Its `LocalLow` deny goes with the
/// profile.
fn remove_user(
    machine: &mut impl Removal,
    user: &PoolUser,
    owner: &Sid,
    report: &mut RemoveReport,
) -> Result<(), String> {
    let failed = |error: MachineError| cause_of(&error);
    if machine.profile_of(&user.sid).map_err(failed)?.is_some() {
        machine.delete_profile(&user.sid, owner).map_err(failed)?;
        report.profiles += 1;
    }
    machine.delete_user(&user.name).map_err(failed)?;
    report.accounts += 1;
    Ok(())
}

/// The group, with its Remote Desktop deny, when it has no members left; whether it went.
/// One with members stays, named in `left`.
fn remove_group(machine: &mut impl Removal, group: &Sid, left: &mut Vec<String>) -> bool {
    let removed = match machine.group_members(group) {
        Ok(0) => machine
            .remove_remote_logon_deny(group)
            .and_then(|()| machine.delete_group(group)),
        Ok(members) => {
            left.push(format!(
                "the group {POOL_GROUP}, which still has {members} member(s)"
            ));
            tracing::warn!(members, "{POOL_GROUP} still has members, so it was left");
            return false;
        }
        Err(error) => Err(error),
    };
    match removed {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "{POOL_GROUP} couldn't be removed");
            left.push(format!("the group {POOL_GROUP}: {}", cause_of(&error)));
            false
        }
    }
}
