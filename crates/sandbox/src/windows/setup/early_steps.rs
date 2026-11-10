//! Set-up's steps before its changes, and what every change is preceded by: step 1's owner
//! check, what an interrupted run left (`intent.rs`), step 2's preflight; the deadline's check
//! before each change (`deadline.rs`) and the intent written before the group and each account.

use super::create::Failed;
use super::intent::{SetUpSide, undo};
use super::machine::{LogonRight, Machine, ServiceStart};
use super::outcome::{
    DEADLINE_REACHED, LOGON_REFUSED, SECLOGON_DISABLED, SetupExit, SetupFailure, Step, cause_of,
};
use super::rollback::Run;
use crate::windows::pool::MachineRecord;
use crate::windows::sids::Sid;

/// Step 1's check: a record owned by another user refuses with that user's SID (D5); one that
/// names the owner as a pool user or the group refuses too, so no step ever touches the owner's
/// own account (S15 `PR4b` review).
pub(super) fn check_owner(record: Option<&MachineRecord>, owner: &Sid) -> Result<(), SetupFailure> {
    let Some(record) = record else {
        return Ok(());
    };
    if record.owner != *owner {
        return Err(SetupFailure::early(
            Step::Owner,
            format!(
                "this computer's protection was set up by another Windows user, {}",
                record.owner
            ),
        ));
    }
    if record.pool.contains(owner) || record.group == *owner {
        // This user owns the set-up, so not `OtherOwner`: a failure with its cause.
        return Err(SetupFailure {
            step: Step::Owner,
            cause: format!(
                "the machine's record names the owner, {owner}, as a sandbox user or group"
            ),
            rolled_back: true,
            exit: SetupExit::Failed,
        });
    }
    Ok(())
}

/// Step 2: Secondary Logon must not be disabled, and local policy must not refuse `Users` the
/// interactive logon right. Both are checked before any account exists.
pub(super) fn preflight(machine: &mut impl Machine) -> Result<(), SetupFailure> {
    match machine.seclogon() {
        Ok(ServiceStart::Automatic | ServiceStart::Manual) => {}
        Ok(ServiceStart::Disabled) => {
            return Err(SetupFailure::early(Step::SecondaryLogon, SECLOGON_DISABLED));
        }
        Err(error) => return Err(SetupFailure::early(Step::SecondaryLogon, cause_of(&error))),
    }
    match machine.logon_right() {
        Ok(LogonRight::Allowed) => Ok(()),
        Ok(LogonRight::Refused) => Err(SetupFailure::early(Step::LogonRight, LOGON_REFUSED)),
        Err(error) => Err(SetupFailure::early(Step::LogonRight, cause_of(&error))),
    }
}

/// After step 1, before anything else changes: what an interrupted run left, as `intent.rs`
/// says. Nothing pending and no partial record is `before` as it is. A partial record another
/// user's set-up left refuses ([`SetupExit::OtherOwner`]), as a whole one does. Otherwise the
/// accounts and the group the intent names and `before` doesn't are undone, then the intent
/// cleared, or, with no whole record, the partial one deleted; the record the run goes on from.
pub(super) fn recover(
    machine: &mut impl Machine,
    before: Option<MachineRecord>,
    owner: &Sid,
) -> Result<Option<MachineRecord>, SetupFailure> {
    let intent = machine
        .read_intent()
        .map_err(|error| SetupFailure::early(Step::ReadRecord, cause_of(&error)))?;
    if intent.is_empty() {
        return Ok(before);
    }
    if before.is_none()
        && let Some(other) = intent.owner.as_ref().filter(|other| *other != owner)
    {
        return Err(SetupFailure::early(
            Step::Owner,
            format!("this computer's protection was being set up by another Windows user, {other}"),
        ));
    }
    let failed = |cause: String, exit: SetupExit| SetupFailure {
        step: Step::Pending,
        cause,
        rolled_back: true,
        exit,
    };
    if machine.cancelled() {
        return Err(failed(
            DEADLINE_REACHED.to_owned(),
            SetupExit::DeadlineReached,
        ));
    }
    let undone = undo(&mut SetUpSide(machine), &intent, before.as_ref())
        .map_err(|cause| failed(cause, SetupExit::Failed))?;
    tracing::warn!(
        users = undone.users,
        group = undone.group,
        "an interrupted set-up's leftovers were undone"
    );
    let cleared = if before.is_some() {
        machine.write_intent(owner, false, &[])
    } else {
        machine.delete_record()
    };
    cleared.map_err(|error| failed(cause_of(&error), SetupExit::Failed))?;
    Ok(before)
}

/// Before each change: once the run's deadline is near (`Machine::cancelled`), the run stops
/// here, at `step`, and its rollback runs; `run` remembers why, for the exit.
pub(super) fn checkpoint(machine: &impl Machine, run: &mut Run, step: Step) -> Result<(), Failed> {
    if machine.cancelled() {
        tracing::warn!(%step, "the set-up stopped for its deadline; rolling back");
        run.deadline = true;
        return Err((step, DEADLINE_REACHED.to_owned()));
    }
    Ok(())
}

/// Before the group (`group`) or an account (`users`): the intent naming it, with the group
/// still pending while no record names the one this run made. Marked in `run` before the
/// write, so a write that fails part way is cleaned up too.
pub(super) fn intend(
    machine: &mut impl Machine,
    run: &mut Run,
    group: bool,
    users: &[String],
) -> Result<(), String> {
    let group = group || run.intent == Some(true);
    run.intent = Some(group);
    let owner = machine.owner().clone();
    machine
        .write_intent(&owner, group, users)
        .map_err(|error| cause_of(&error))
}

/// Once the record names what the intent named: the intent cleared. Nothing to do when this run
/// has none outstanding.
pub(super) fn recorded_intent(machine: &mut impl Machine, run: &mut Run) -> Result<(), Failed> {
    if run.intent.is_none() {
        return Ok(());
    }
    let owner = machine.owner().clone();
    machine
        .write_intent(&owner, false, &[])
        .map_err(|error| (Step::WriteRecord, cause_of(&error)))?;
    run.intent = None;
    Ok(())
}
