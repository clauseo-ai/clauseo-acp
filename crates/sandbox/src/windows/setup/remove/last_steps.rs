//! The removal's last steps (split from `remove.rs`): the group, the owner's markers and a
//! per-machine install's stop and sweep tasks; and what an interrupted set-up left (`intent.rs`),
//! with a whole record ([`leftovers`]) or only a partial one ([`partial`]).

use super::super::intent::{RemovalSide, undo};
use super::super::record_values::CopyOwner;
use super::{
    Failed, MachineError, REMOVE_DEADLINE, Removal, RemoveFailure, RemoveReport, RemoveStep,
    Removed, SetupExit, cause_of,
};
use crate::windows::pool::MachineRecord;

/// With no whole record, under the lock: a partial one an interrupted set-up left, and what its
/// intent names, as `intent.rs` says. Nothing there is [`Removed::NothingSetUp`]. A per-machine
/// removal, or the owner the intent names, undoes it and deletes the partial record; another
/// user's per-user removal touches nothing ([`Removed::NotOwner`], or `NothingSetUp` when no
/// owner is named); a user who can't be found gets `Failed`, nothing touched, as with a whole
/// record.
pub(super) fn partial(
    machine: &mut impl Removal,
    per_machine: bool,
) -> Result<Removed, RemoveFailure> {
    let failure =
        |step: RemoveStep, cause: String, exit: SetupExit| RemoveFailure { step, cause, exit };
    let intent = machine
        .read_intent()
        .map_err(|e| failure(RemoveStep::ReadRecord, cause_of(&e), SetupExit::Failed))?;
    if intent.is_empty() {
        return Ok(Removed::NothingSetUp);
    }
    if !per_machine {
        match (machine.invoker(), &intent.owner) {
            (Some(invoker), Some(owner)) if invoker == owner => {}
            (None, _) => {
                return Err(failure(
                    RemoveStep::Owner,
                    "the user who asked can't be found, so nothing was removed".to_owned(),
                    SetupExit::Failed,
                ));
            }
            (Some(_), Some(_)) => return Ok(Removed::NotOwner),
            (Some(_), None) => return Ok(Removed::NothingSetUp),
        }
    }
    if machine.cancelled() {
        let cause = REMOVE_DEADLINE.to_owned();
        return Err(failure(
            RemoveStep::Pending,
            cause,
            SetupExit::DeadlineReached,
        ));
    }
    let partly = |step: RemoveStep, cause: String| failure(step, cause, SetupExit::RemovedPartly);
    let undone = undo(&mut RemovalSide(machine), &intent, None)
        .map_err(|cause| partly(RemoveStep::Pending, cause))?;
    machine
        .delete_record()
        .map_err(|e| partly(RemoveStep::Record, cause_of(&e)))?;
    Ok(Removed::All(RemoveReport {
        accounts: undone.users,
        group: undone.group,
        ..RemoveReport::default()
    }))
}

/// With a whole record, once its pool users are gone and before its group: the accounts and the
/// group the intent names and the record doesn't (a set-up stopped between making one and
/// recording it), which would otherwise keep the group from going. The intent itself goes with
/// the record.
pub(super) fn leftovers(
    machine: &mut impl Removal,
    record: &MachineRecord,
    report: &mut RemoveReport,
) -> Result<(), Failed> {
    let intent = machine
        .read_intent()
        .map_err(|e| (RemoveStep::Pending, cause_of(&e)))?;
    let undone = undo(&mut RemovalSide(machine), &intent, Some(record))
        .map_err(|cause| (RemoveStep::Pending, cause))?;
    report.accounts += undone.users;
    Ok(())
}

/// The group, when it is the record's and empty; whether it was deleted. Gone already is
/// `false`; another group under the name, or one with members, stops the run. Its Remote
/// Desktop deny comes off just before the delete ([`RemoveStep::RemoteLogon`] when it won't):
/// a user right is keyed by SID, and would outlive the group. With the group gone already,
/// the deny still comes off by the record's `GroupSid`, as an earlier run may have left it.
pub(super) fn remove_group(
    machine: &mut impl Removal,
    record: &MachineRecord,
) -> Result<bool, Failed> {
    let failed = |e: MachineError| (RemoveStep::Group, cause_of(&e));
    let Some(group) = machine.group().map_err(failed)? else {
        machine
            .remove_remote_logon_deny(&record.group)
            .map_err(|e| (RemoveStep::RemoteLogon, cause_of(&e)))?;
        return Ok(false);
    };
    if group != record.group {
        return Err((
            RemoveStep::Group,
            "the group ClauseoSandboxUsers isn't the one the record names, so it is left alone"
                .to_owned(),
        ));
    }
    match machine.group_members(&group).map_err(failed)? {
        0 => {
            machine
                .remove_remote_logon_deny(&group)
                .map_err(|e| (RemoveStep::RemoteLogon, cause_of(&e)))?;
            machine.delete_group(&group).map_err(failed).map(|()| true)
        }
        members => Err((
            RemoveStep::Group,
            format!("the group ClauseoSandboxUsers still has {members} members"),
        )),
    }
}

/// Step 8, as the owner: the unclean-session markers in the data folder `owners_ledger` checked.
pub(super) fn clear_markers(machine: &mut impl Removal) -> Result<(), Failed> {
    match machine.as_owner(Removal::clear_markers) {
        Ok(Ok(_cleared)) => Ok(()),
        Ok(Err(error)) | Err(error) => Err((RemoveStep::Markers, cause_of(&error))),
    }
}

/// Step 8, before the record goes: on a per-machine install (`AdminCopyOwner` is `machine`),
/// the tasks `\Clauseo\SandboxStop` and `\Clauseo\SandboxSweep` set-up registered for the
/// record's owner are deleted, so the owner keeps no start right once their pool is gone. A
/// per-user install's tasks stay (`--adopt` registered them; `--remove --uninstall` deletes
/// them with the admin copy).
pub(super) fn remove_stop_task(machine: &mut impl Removal) -> Result<(), Failed> {
    let failed = |e: MachineError| (RemoveStep::StopTask, cause_of(&e));
    match machine.copy_owner().map_err(failed)? {
        Some(CopyOwner::Machine) => machine.unregister_stop_task().map_err(failed),
        Some(CopyOwner::User(_)) | None => Ok(()),
    }
}
