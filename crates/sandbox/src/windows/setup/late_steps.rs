//! Set-up's steps around the denies and the matters: the owners of the folders the shared-write
//! denies protect (`PR4h`), reading the owner's registered matters after the lock (`PR4h`), step
//! 8's fence of them against the grown pool, step 9's change journals, and the SYSTEM tasks
//! once the owner is recorded.

use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::create::Failed;
use super::journal_plan::{JournalAction, journal_plan};
use super::machine::{Ensured, Machine, MachineError, TASK_REENABLED, owned};
use super::outcome::{JournalOutcome, SetupFailure, Step, cause_of};
use super::record_values::CopyOwner;
use super::rollback::Run;
use crate::Error;
use crate::windows::apply::{Applied, fence_each};
use crate::windows::fence::RegisteredMatter;
use crate::windows::pool::MachineRecord;
use crate::windows::sids::Sid;
use crate::windows::wfp::{NetworkReport, NetworkSetup};
use crate::windows::writer::AclWriter;

/// `SYSTEM`.
const SYSTEM: &str = "S-1-5-18";
/// `BUILTIN\Administrators`.
const ADMINISTRATORS: &str = "S-1-5-32-544";

/// After step 5, once the record names `owner` as `OwnerSid`, the SYSTEM tasks are registered
/// for `owner`, or replaced when there, by `refresh::register_task`: per user (the record's
/// `AdminCopyOwner` is `owner`), `\Clauseo\SandboxRefresh` then `\Clauseo\SandboxStop`
/// (`refresh::TASKS`' order), as plain `--adopt` registers them; per machine (it is `machine`),
/// the stop task alone, since IT updates a per-machine install (0049). Each descriptor then names
/// `owner` alone, so a previous owner's start right goes with the replaced task, and a re-run
/// registers them again.
///
/// Per user this is what makes the tasks once `--adopt --place-only` has placed the copy: the
/// app's first run runs this set-up from the admin copy, outside the installer's process tree
/// (0049, "Admin copy"). A copy another user placed keeps that user's tasks: nothing is
/// registered for `owner` then. An install with no admin copy has no task, so nothing is
/// registered either. A record that can't be read fails at [`Step::StopTask`]; a registration
/// that fails at [`Step::RefreshTask`] or [`Step::StopTask`]; each rolled back.
///
/// An elevated set-up then registers the set-up task again when it is missing or its definition
/// differs (`Machine::ensure_set_up_task`): per user for `owner`, per machine for interactive
/// users with the pool's `group` refused. So a lawyer whose task was deleted meets the prompt
/// once, not on every set-up. One it found disabled is written to the Event Log
/// ([`TASK_REENABLED`]), so IT sees it turned back on. The set-up task's own run doesn't: per
/// machine it gives the task the deny of `group`, which may exist only since this run
/// (`Machine::deny_set_up_to`, every
/// run, as a reinstall gives the task its install-time descriptor), as a pool user holds `IU`
/// too; per user the task names the owner alone and needs nothing. A failure is
/// [`Step::SetUpTask`]; a task run on an install with no set-up task skips the deny. The set-up
/// task's server refuses a pool user whatever the descriptor says (`setup::serve`), so the deny
/// is a second wall, not the only one.
pub(super) fn tasks(machine: &mut impl Machine, owner: &Sid, group: &Sid) -> Result<(), Failed> {
    let refresh = match machine.copy_owner() {
        Ok(Some(CopyOwner::Machine)) => false,
        Ok(Some(CopyOwner::User(placed))) if placed == *owner => true,
        Ok(Some(CopyOwner::User(_))) => {
            tracing::warn!("another user's install placed the admin copy; its tasks stay");
            return Ok(());
        }
        Ok(None) => return Ok(()),
        Err(error) => {
            return Err((
                Step::StopTask,
                format!(
                    "the tasks: the record's AdminCopyOwner can't be read ({})",
                    cause_of(&error)
                ),
            ));
        }
    };
    let failed = |step: Step, name: &str, error: &MachineError| {
        (step, format!("the task {name}: {}", cause_of(error)))
    };
    if refresh {
        machine
            .register_refresh_task(owner)
            .map_err(|error| failed(Step::RefreshTask, "SandboxRefresh", &error))?;
    }
    machine
        .register_stop_task(owner)
        .map_err(|error| failed(Step::StopTask, "SandboxStop", &error))?;
    let per_machine = (!refresh).then_some(group);
    let ensured = machine
        .ensure_set_up_task(owner, per_machine)
        .map_err(|error| failed(Step::SetUpTask, "SandboxSetUp", &error))?;
    if ensured == Some(Ensured::ReEnabled) {
        // A crashed uninstall or IT disabled it: IT sees that this set-up turned it back on.
        machine.event_log(TASK_REENABLED);
    }
    let healed = ensured.is_some();
    if !refresh && !healed {
        let denied = machine
            .deny_set_up_to(group)
            .map_err(|error| failed(Step::SetUpTask, "SandboxSetUp", &error))?;
        if !denied {
            tracing::info!("this per-machine install has no set-up task to keep the pool from");
        }
    }
    Ok(())
}

/// Before step 7: the folders whose denies protect what is below them, which an owner could
/// undo through its implied `WRITE_DAC` (each pool user's `AppData\LocalLow`, whose deny for the
/// group is inherited and whose `OWNER RIGHTS` deny is inherit-only). A fresh profile's
/// `LocalLow` can be owned by whichever account loaded it, so each folder's owner is read on
/// the handle that reaches it from its profile (`Machine::owner_in_profile`), and one that isn't
/// `SYSTEM`, `Administrators` or the set-up's own account (a pool user, say) is handed to
/// `Administrators` (`Machine::give_to_admins`) and read again the same way. Still anyone
/// else's, or unreadable, fails at [`Step::Owners`] naming the folder, before any deny is
/// written. A missing folder is skipped here: step 7's deny on it fails at
/// [`Step::SharedWrites`], which names it as missing.
pub(super) fn own_protected(
    machine: &mut impl Machine,
    folders: &[(PathBuf, PathBuf)],
) -> Result<(), Failed> {
    let failed =
        |folder: &Path, why: String| (Step::Owners, format!("{}: {why}", folder.display()));
    for (profile, folder) in folders {
        let owner = machine
            .owner_in_profile(profile, folder)
            .map_err(|error| failed(folder, cause_of(&error)))?;
        let Some(owner) = owner else {
            continue;
        };
        if allowed_owner(machine, &owner) {
            continue;
        }
        machine.give_to_admins(profile, folder).map_err(|error| {
            failed(
                folder,
                format!("its owner {owner} can't be replaced: {}", cause_of(&error)),
            )
        })?;
        let now = machine
            .owner_in_profile(profile, folder)
            .map_err(|error| failed(folder, cause_of(&error)))?
            .ok_or_else(|| failed(folder, "it went missing".to_owned()))?;
        if !allowed_owner(machine, &now) {
            return Err(failed(
                folder,
                format!("still owned by {now}, not Administrators"),
            ));
        }
    }
    Ok(())
}

/// Before step 7's deny on `DeviceSync` (`folder`): its owner is Windows' (`SYSTEM` or
/// `TrustedInstaller`), never handed over, only checked. One of `pool` (a pool user or the
/// group) owning it fails at [`Step::Owners`]: it could remove the deny it would get.
pub(super) fn refuse_pool_owner(
    machine: &mut impl Machine,
    folder: &Path,
    pool: &[Sid],
) -> Result<(), Failed> {
    let owner = machine.owner_of(folder).map_err(|error| {
        (
            Step::Owners,
            format!("{}: {}", folder.display(), cause_of(&error)),
        )
    })?;
    if pool.contains(&owner) {
        return Err((
            Step::Owners,
            format!("{} is owned by the sandbox user {owner}", folder.display()),
        ));
    }
    Ok(())
}

/// Whether `owner` may own a protected folder: `SYSTEM`, `Administrators`, or the account the
/// set-up runs as (never a pool user: the record refuses a pool SID that is the owner's).
fn allowed_owner(machine: &impl Machine, owner: &Sid) -> bool {
    [SYSTEM, ADMINISTRATORS].contains(&owner.as_str()) || machine.own_account() == Some(owner)
}

/// The owner's registered matters (`Machine::registered_matters`), read as the owner once the
/// lock is held, so no other set-up changes the record, the ledger or the pool between this read
/// and the fence. The record's pool users and group are trusted in the ledger. A file that
/// can't be read fails closed at [`Step::Matters`]; a missing one is no matters.
pub(super) fn registered(
    machine: &mut impl Machine,
    record: Option<&MachineRecord>,
) -> Result<Vec<RegisteredMatter>, SetupFailure> {
    let mut trusted: Vec<Sid> = record.map(|r| r.pool.clone()).unwrap_or_default();
    trusted.extend(record.map(|r| r.group.clone()));
    owned(machine, |m| m.registered_matters(&trusted)).map_err(|error| {
        SetupFailure::early(
            Step::Matters,
            format!("the registered matters: {}", cause_of(&error)),
        )
    })
}

/// What step 8 did: the fences applied, and each matter skipped with why.
pub(super) type Fenced = (Applied, Vec<(MatterId, String)>);

/// Step 8: when this run made users and matters are registered (an extend, or a re-run that made
/// a user again), every registered matter is fenced against the grown pool through the owner's
/// ledger, which the rollback reads after. The whole step runs as the owner: the ledger is the
/// owner's file, and the matters' folders are the owner's, so the elevated process writes there
/// only what the owner could (K1). Matters are fenced one at a time (`fence_each`): a pool
/// user's root goes to the owner first; one whose hand-back fails is skipped and reported, the
/// others fenced; any other failure fails [`Step::Fence`].
pub(super) fn fence(
    machine: &mut impl Machine,
    acl: &mut impl AclWriter,
    matters: &[RegisteredMatter],
    run: &mut Run,
    group: &Sid,
    live: &[Sid],
) -> Result<Option<Fenced>, Failed> {
    if run.made.is_empty() || matters.is_empty() {
        return Ok(None);
    }
    let mut trusted: Vec<Sid> = run.recorded().map(|r| r.pool.clone()).unwrap_or_default();
    trusted.push(group.clone());
    let failed = |cause: String| (Step::Fence, cause);
    let (ledger, fenced) = machine
        .as_owner(|machine| {
            let mut ledger = machine.ledger(&trusted)?;
            let around = machine.surroundings();
            let fenced = fence_each(&mut ledger, matters, live, acl, around);
            Ok::<_, io::Error>((ledger, fenced))
        })
        .map_err(|error| failed(cause_of(&error)))?
        .map_err(|error| failed(format!("the permission ledger: {error}")))?;
    run.ledger = Some(ledger);
    let fenced = fenced.map_err(|error| failed(error.to_string()))?;
    let skipped = fenced
        .skipped
        .into_iter()
        .map(|(matter, error)| {
            let why = if let Error::AclApply { at: Some(at), .. } = &error {
                format!("{}: {error}", at.display())
            } else {
                error.to_string()
            };
            tracing::warn!(%matter, "set-up couldn't fence a matter a sandbox user owns part of");
            (matter, why)
        })
        .collect();
    Ok(Some((fenced.applied, skipped)))
}

/// Step 8a, the network filters (S15 PR7): the block filters, static permits, read grants and
/// firewall rules rewritten for the pool group and every live pool user, `live`, so an extend
/// covers its new users. Runs elevated, as the set-up itself: WFP and the firewall are the
/// machine's, not the owner's files. A WFP failure fails [`Step::Network`]; the filters'
/// transaction leaves the old ones in place then. A firewall that doesn't take effect is only
/// reported (and logged by `wfp::install`).
pub(super) fn network(
    machine: &mut impl Machine,
    owner: &Sid,
    group: &Sid,
    live: &[Sid],
) -> Result<NetworkReport, Failed> {
    let setup = NetworkSetup::new(owner, group, live)
        .map_err(|error| (Step::Network, error.to_string()))?;
    machine
        .network(&setup)
        .map_err(|error| (Step::Network, cause_of(&error)))
}

/// Step 9: each chosen volume's change journal made [`super::JOURNAL_MAX`] when smaller
/// ([`journal_plan`], D-B12).
/// Failures are listed, never fatal: they cost only full walks later.
pub(super) fn journals(
    machine: &mut impl Machine,
    matters: &[RegisteredMatter],
) -> Result<Vec<(PathBuf, JournalOutcome)>, String> {
    let roots: Vec<&Path> = matters
        .iter()
        .map(|matter| matter.folders.root.as_path())
        .collect();
    let volumes = machine
        .journal_volumes(&roots)
        .map_err(|error| cause_of(&error))?;
    let mut done = Vec::new();
    for volume in volumes {
        let outcome = match machine.journal_size(&volume) {
            Ok(size) => match journal_plan(size) {
                JournalAction::Leave => JournalOutcome::Left(size.unwrap_or_default()),
                JournalAction::Grow { max, delta } => {
                    match machine.grow_journal(&volume, max, delta) {
                        Ok(()) => JournalOutcome::Grown,
                        Err(error) => JournalOutcome::Failed(cause_of(&error)),
                    }
                }
            },
            Err(error) => JournalOutcome::Failed(cause_of(&error)),
        };
        if let JournalOutcome::Failed(why) = &outcome {
            tracing::warn!(volume = %volume.display(), %why, "the change journal wasn't enlarged");
        }
        done.push((volume, outcome));
    }
    Ok(done)
}
