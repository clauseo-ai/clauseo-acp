//! `--remove`'s side of `stranded.rs`: the pool accounts the record doesn't list, proven
//! Clauseo's, removed; the rest named and left.
//!
//! - [`widen`]: a record whose `OwnerSid`, `GroupSid` and `DataDir` read, but whose pool lost
//!   users (blanked, cut short, or malformed and read as empty): each proven account the group
//!   named by `GroupSid` holds is written back into the record, with `Removing`, and then goes
//!   the way every recorded user goes (`remove.rs` step 4), its profile and rights included.
//! - [`sweep`]: no record, or one that can't be trusted ([`MachineError::RecordCorrupt`]). A
//!   corrupt record whose `OwnerSid` still reads is decided as a record is ([`decide_corrupt`],
//!   D-B9: a per-user removal by anyone else touches nothing). The local group is found by name
//!   (only an administrator can make one); each proven member is stopped, disabled
//!   (`UF_ACCOUNTDISABLE`, so a delete that fails leaves an account that can't log on), and
//!   goes as a recorded user goes: its profile with its `ProfileList` entry, the account with
//!   its sign-in screen value, every user right its SID holds, its scheduled tasks. A proven
//!   account left is [`SetupExit::RemovedPartly`]: the next run finds it again by the same
//!   proof. Then, for a corrupt record, what `remove.rs` steps 4 to 9 take off around the pool:
//!   the network filters (the owner's read grant only when `OwnerSid` reads), `DeviceSync`'s
//!   deny (by `GroupSid`, or the group found), the ledger's entries (only when `OwnerSid` and
//!   `DataDir` read), the group once empty, the owner's markers (as the owner only), the stop
//!   and sweep tasks, and last the record's values. Nothing is handed to anyone: the SIDs die
//!   with their accounts.

use super::super::machine::MachineError;
use super::super::outcome::{SetupExit, cause_of};
use super::super::stranded::{PartialRecord, log_doubtful, sort};
use super::{
    Acting, Decision, Removal, RemoveFailure, RemoveReport, RemoveStep, Removed, clear_markers,
    decide, ledger_work, remove_denies, remove_stop_task,
};
use crate::windows::pool::{MachineRecord, POOL_GROUP, PoolUser};
use crate::windows::shared_writes::shared_write_targets;
use crate::windows::sids::Sid;
use crate::windows::writer::AclWriter;

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

/// For a record that is gone (`corrupt` false): `Ok(None)`, the sweep alone. For a corrupt one:
/// who acts, decided from its `OwnerSid` when that reads as `remove.rs` decides a record's
/// (`Err` with what to end with, a per-user removal by anyone else included), else as an
/// administrator.
pub(super) fn decide_corrupt(
    machine: &mut impl Removal,
    corrupt: bool,
    per_machine: bool,
) -> Result<Result<Option<Acting>, Removed>, RemoveFailure> {
    if !corrupt {
        return Ok(Ok(None));
    }
    let Some(owner) = machine.read_partial().owner else {
        return Ok(Ok(Some(Acting::Admin)));
    };
    Ok(match decide(machine, Some(&owner), per_machine)? {
        Decision::Stop(removed) => Err(removed),
        Decision::Act(acting) => Ok(Some(acting)),
    })
}

/// `record` with every proven account it doesn't list added to its pool, written with
/// `Removing` set when any was, the doubtful accounts' names, and this machine's account
/// domain (the only one whose SIDs lose their rights, `remove.rs` step 4).
pub(super) fn widen(
    machine: &mut impl Removal,
    record: MachineRecord,
) -> Result<(MachineRecord, Vec<String>, [u32; 3]), RemoveFailure> {
    let accounts = machine.pool_accounts().map_err(|error| RemoveFailure {
        step: RemoveStep::ReadRecord,
        cause: cause_of(&error),
        exit: SetupExit::Failed,
    })?;
    let mut skip = record.pool.clone();
    skip.push(record.owner.clone());
    let mut sorted = sort(&accounts, &skip);
    if sorted.group.as_ref() != Some(&record.group) {
        sorted.proven.clear();
    }
    log_doubtful(&sorted.doubtful, &accounts.foreign);
    let left = sorted.doubtful.iter().map(ToString::to_string).collect();
    if sorted.proven.is_empty() {
        return Ok((record, left, accounts.machine));
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
    Ok((widened, left, accounts.machine))
}

/// With no record to trust, as the module says. `corrupt` is who acts for a corrupt record,
/// `None` for one that is gone. [`Removed::NothingSetUp`] when there was nothing (no group, no
/// record); else [`Removed::All`], with what was left named in [`RemoveReport::left`].
pub(super) fn sweep(
    machine: &mut impl Removal,
    acl: &mut impl AclWriter,
    corrupt: Option<Acting>,
) -> Result<Removed, RemoveFailure> {
    let accounts = machine.pool_accounts().map_err(|error| RemoveFailure {
        step: RemoveStep::ReadRecord,
        cause: cause_of(&error),
        exit: SetupExit::Failed,
    })?;
    let partial = match corrupt {
        Some(_) => machine.read_partial(),
        None => PartialRecord::default(),
    };
    let asker = machine.invoker().cloned();
    let owner = match partial.owner.clone().or_else(|| asker.clone()) {
        Some(owner) => owner,
        None => Sid::parse(SYSTEM).map_err(|error| RemoveFailure {
            step: RemoveStep::Owner,
            cause: error.to_string(),
            exit: SetupExit::Failed,
        })?,
    };
    let skip: Vec<Sid> = [Some(owner.clone()), asker].into_iter().flatten().collect();
    let sorted = sort(&accounts, &skip);
    log_doubtful(&sorted.doubtful, &accounts.foreign);
    let mut report = RemoveReport {
        left: sorted.doubtful.iter().map(ToString::to_string).collect(),
        ..RemoveReport::default()
    };
    if sorted.group.is_none() && corrupt.is_none() {
        return Ok(Removed::NothingSetUp);
    }
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
    let mut remaining = Vec::new();
    for user in &sorted.proven {
        tracing::warn!(account = %user.name, "a sandbox user no record lists is removed");
        if let Err(left) = remove_user(machine, user, &owner, &mut report) {
            let cause = &left.cause;
            tracing::warn!(account = %user.name, %cause, "a sandbox user couldn't be removed");
            remaining.push(left.to_string());
        }
    }
    if !pool.is_empty()
        && let Err(error) = machine.sweep_tasks(&pool)
    {
        tracing::warn!(%error, "the removed sandbox users' tasks weren't swept");
        report
            .left
            .push(format!("their scheduled tasks: {}", cause_of(&error)));
    }
    if !remaining.is_empty() {
        return Err(RemoveFailure {
            step: RemoveStep::Account,
            cause: format!(
                "sandbox users Clauseo set up couldn't be removed: {}; uninstall again to finish",
                remaining.join("; ")
            ),
            exit: SetupExit::RemovedPartly,
        });
    }
    let partly = |(step, cause): (RemoveStep, String)| RemoveFailure {
        step,
        cause,
        exit: SetupExit::RemovedPartly,
    };
    if let Some(acting) = corrupt {
        around_the_pool(
            machine,
            acl,
            &partial,
            sorted.group.as_ref(),
            &pool,
            acting,
            &mut report,
        )
        .map_err(partly)?;
    }
    if let Some(group) = &sorted.group {
        report.group = remove_group(machine, group, &mut report.left);
    }
    if let Some(acting) = corrupt {
        if acting == Acting::Owner {
            clear_markers(machine).map_err(partly)?;
        }
        remove_stop_task(machine).map_err(partly)?;
        machine
            .delete_record()
            .map_err(|error| partly((RemoveStep::Record, cause_of(&error))))?;
    }
    Ok(Removed::All(report))
}

/// For a corrupt record, `remove.rs` steps 4 to 6 around the pool: the network, `DeviceSync`'s
/// deny for the group (`GroupSid`, or the group found), and the ledger's entries when
/// `OwnerSid`, `GroupSid` and `DataDir` read (else left: they name SIDs nothing has, K10).
fn around_the_pool(
    machine: &mut impl Removal,
    acl: &mut impl AclWriter,
    partial: &PartialRecord,
    found: Option<&Sid>,
    pool: &[Sid],
    acting: Acting,
    report: &mut RemoveReport,
) -> Result<(), (RemoveStep, String)> {
    report.firewall = machine
        .remove_network(partial.owner.as_ref())
        .map_err(|e| (RemoveStep::Network, cause_of(&e)))?;
    let Some(group) = partial.group.clone().or_else(|| found.cloned()) else {
        return Ok(());
    };
    let program_data = machine
        .program_data()
        .map_err(|e| (RemoveStep::DeviceSync, cause_of(&e)))?;
    let device_sync = shared_write_targets(&program_data, &[]);
    remove_denies(acl, &group, &device_sync).map_err(|cause| {
        (
            RemoveStep::DeviceSync,
            format!("DeviceSync's deny: {cause}"),
        )
    })?;
    let (Some(owner), Some(data_dir)) = (&partial.owner, &partial.data_dir) else {
        tracing::warn!("the corrupt record names no owner or data folder; the ledger stays");
        return Ok(());
    };
    let record = MachineRecord {
        owner: owner.clone(),
        group,
        pool: pool.to_vec(),
        data_dir: data_dir.clone(),
        removing: true,
    };
    report.ledger = ledger_work::run(machine, acl, &record, acting)?;
    Ok(())
}

/// A proven account left, why, and whether it was disabled.
struct Left<'a> {
    /// The account's name.
    name: &'a str,
    /// Why it is still there.
    cause: String,
    /// Why it couldn't be disabled, or `None` when it was.
    not_disabled: Option<String>,
}

impl std::fmt::Display for Left<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.not_disabled {
            None => write!(f, "{} (disabled): {}", self.name, self.cause),
            Some(why) => write!(f, "{} (not disabled: {why}): {}", self.name, self.cause),
        }
    }
}

/// One proven account as a recorded one goes: disabled first, then its profile
/// (`DeleteProfileW`, its folder and `ProfileList` entry; its `LocalLow` deny with it), the
/// account with its sign-in screen value, then every user right its SID still holds. `Err`
/// when the account is left, saying whether it was disabled; a right that won't come off is
/// named in `left` only.
fn remove_user<'a>(
    machine: &mut impl Removal,
    user: &'a PoolUser,
    owner: &Sid,
    report: &mut RemoveReport,
) -> Result<(), Left<'a>> {
    let not_disabled = machine.disable(&user.name).err().map(|error| {
        tracing::warn!(account = %user.name, %error, "a sandbox user couldn't be disabled");
        cause_of(&error)
    });
    let failed = |error: MachineError| Left {
        name: &user.name,
        cause: cause_of(&error),
        not_disabled: not_disabled.clone(),
    };
    if machine.profile_of(&user.sid).map_err(failed)?.is_some() {
        machine.delete_profile(&user.sid, owner).map_err(failed)?;
        report.profiles += 1;
    }
    machine.delete_user(&user.name).map_err(failed)?;
    report.accounts += 1;
    if let Err(error) = machine.revoke_rights(&user.sid) {
        tracing::warn!(account = %user.name, %error, "a removed user's rights stay on its SID");
        report
            .left
            .push(format!("{}'s user rights: {}", user.name, cause_of(&error)));
    }
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
