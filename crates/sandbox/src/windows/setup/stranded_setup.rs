//! Set-up's side of `stranded.rs`: before the plan, the accounts a lost or untrusted record
//! stranded are removed ([`record_or_sweep`]); with a trusted record, the proven accounts its
//! pool lost are adopted back ([`adoptable`]).

use super::machine::{Machine, MachineError};
use super::outcome::{SetupFailure, Step, cause_of};
use super::stranded::{log_doubtful, sort, unprovable_members};
use crate::windows::pool::{MachineRecord, POOL_GROUP, PoolUser};
use crate::windows::sids::Sid;

/// The record set-up starts from: the record when it can be trusted; otherwise, with everything a
/// lost or corrupt record stranded taken out first, none. Each proven account is disabled, then
/// goes as `--remove` takes one: its profile, the account with its sign-in screen value (removed
/// first), every user right its SID holds, then the pool's tasks. A group left with members set-up
/// can't prove are Clauseo's refuses the run, naming them.
pub(super) fn record_or_sweep(
    machine: &mut impl Machine,
) -> Result<Option<MachineRecord>, SetupFailure> {
    match machine.read_record() {
        Ok(Some(record)) => return Ok(Some(record)),
        Ok(None) => {}
        Err(MachineError::RecordCorrupt(why)) => tracing::warn!(
            %why,
            "the machine's record can't be trusted; set-up removes the sandbox users it can \
             prove are Clauseo's and makes a new pool"
        ),
        Err(error) => return Err(SetupFailure::early(Step::ReadRecord, cause_of(&error))),
    }
    let accounts = machine
        .pool_accounts()
        .map_err(|error| SetupFailure::early(Step::Pool, cause_of(&error)))?;
    let skip = [machine.owner().clone()];
    let sorted = sort(&accounts, &skip);
    log_doubtful(&sorted.doubtful, &accounts.foreign);
    let Some(group) = sorted.group.clone() else {
        return Ok(None);
    };
    let failed = |step: Step, error: &MachineError| SetupFailure::early(step, cause_of(error));
    for user in &sorted.proven {
        tracing::warn!(account = %user.name, "a sandbox user no record lists is removed");
        // Disabled first, so a delete that fails leaves an account that can't log on.
        machine
            .disable(&user.name)
            .map_err(|error| failed(Step::CreateUser, &error))?;
        let profile = machine
            .profile_of(&user.sid)
            .map_err(|error| failed(Step::Profile, &error))?;
        if profile.is_some() {
            machine
                .delete_profile(&user.sid)
                .map_err(|error| failed(Step::Profile, &error))?;
        }
        machine
            .delete_user(&user.name)
            .map_err(|error| failed(Step::CreateUser, &error))?;
        if let Err(error) = machine.revoke_rights(&user.sid) {
            tracing::warn!(account = %user.name, %error, "a removed user's rights stay on its SID");
        }
    }
    let pool: Vec<Sid> = sorted.proven.iter().map(|user| user.sid.clone()).collect();
    if !pool.is_empty() {
        machine
            .sweep_tasks(&pool)
            .map_err(|error| failed(Step::Pool, &error))?;
    }
    let left = machine
        .group_members(&group)
        .map_err(|error| failed(Step::Group, &error))?;
    if left > 0 {
        let names = unprovable_members(&sorted, &accounts.foreign);
        return Err(SetupFailure::early(
            Step::Group,
            format!(
                "the group {POOL_GROUP}, which no record names, still has {left} member(s) \
                 set-up can't prove are Clauseo's ({}); take them out of the group, then run \
                 set-up again",
                names.join(", ")
            ),
        ));
    }
    machine
        .remove_remote_logon_deny(&group)
        .map_err(|error| failed(Step::RemoteLogon, &error))?;
    machine
        .delete_group(&group)
        .map_err(|error| failed(Step::Group, &error))?;
    Ok(None)
}

/// The proven accounts `record` doesn't list, for set-up to adopt: none when the group by name
/// isn't the record's (the group step then refuses).
pub(super) fn adoptable(
    machine: &mut impl Machine,
    record: &MachineRecord,
) -> Result<Vec<PoolUser>, SetupFailure> {
    let accounts = machine
        .pool_accounts()
        .map_err(|error| SetupFailure::early(Step::Pool, cause_of(&error)))?;
    let mut skip = record.pool.clone();
    skip.push(record.owner.clone());
    let mut sorted = sort(&accounts, &skip);
    if sorted.group.as_ref() != Some(&record.group) {
        sorted.proven.clear();
    }
    log_doubtful(&sorted.doubtful, &accounts.foreign);
    for user in &sorted.proven {
        tracing::warn!(account = %user.name, "a sandbox user the record lost is adopted back");
    }
    Ok(sorted.proven)
}
