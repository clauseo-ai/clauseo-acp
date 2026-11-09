//! Set-up's steps around the denies and the matters: the owners of the folders the shared-write
//! denies protect (`PR4h`), reading the owner's registered matters after the lock (`PR4h`), step
//! 8's fence of them against the grown pool, WSL's deny (after the record, and again after the
//! marker), step 9's change journals, and the per-machine stop, sweep and WSL tasks once the
//! owner is recorded.

use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::create::Failed;
use super::journal_plan::{JournalAction, journal_plan};
use super::machine::{Machine, owned};
use super::outcome::{JournalOutcome, SetupFailure, Step, cause_of};
use super::record_values::CopyOwner;
use super::rollback::Run;
use crate::Error;
use crate::windows::apply::{Applied, fence_each};
use crate::windows::fence::RegisteredMatter;
use crate::windows::pool::MachineRecord;
#[cfg(any(windows, test))]
use crate::windows::refresh::MACHINE_TASKS;
use crate::windows::sids::Sid;
use crate::windows::wfp::{NetworkReport, NetworkSetup};
use crate::windows::writer::AclWriter;
use crate::windows::wsl::WslApplied;

/// `SYSTEM`.
const SYSTEM: &str = "S-1-5-18";
/// `BUILTIN\Administrators`.
const ADMINISTRATORS: &str = "S-1-5-32-544";

/// After step 5, once the record names `owner` as `OwnerSid`: on a per-machine install (the
/// record's `AdminCopyOwner` is `machine`), each of [`MACHINE_TASKS`] (`\Clauseo\SandboxStop`,
/// `\Clauseo\SandboxSweep`, `\Clauseo\SandboxWslDeny`, then `\Clauseo\SandboxWslUpgrade`) is
/// registered for `owner`, or replaced when it is there, by the same registration `--adopt`
/// makes per user (`refresh::register_task`). Its descriptor then names `owner` alone, so a
/// previous owner's start right goes with the replaced one. A per-user install's tasks are
/// `--adopt`'s, and an install with no admin copy has none, so neither is touched here. A record
/// that can't be read or a registration that fails is [`Step::StopTask`], rolled back.
pub(super) fn machine_tasks(machine: &mut impl Machine, owner: &Sid) -> Result<(), Failed> {
    let failed = |name: &str, why: String| (Step::StopTask, format!("the task {name}: {why}"));
    match machine.copy_owner() {
        Ok(Some(CopyOwner::Machine)) => {
            // The task definitions build on Windows and in the tests only.
            #[cfg(not(any(windows, test)))]
            let _ = owner;
            #[cfg(any(windows, test))]
            for task in MACHINE_TASKS {
                machine
                    .register_task(task, owner)
                    .map_err(|error| failed(task.name(), cause_of(&error)))?;
            }
            Ok(())
        }
        Ok(Some(CopyOwner::User(_)) | None) => Ok(()),
        Err(error) => Err(failed(
            "SandboxStop",
            format!(
                "the record's AdminCopyOwner can't be read ({})",
                cause_of(&error)
            ),
        )),
    }
}

/// Right after step 5 records the group: on a machine with an admin copy (the record names its
/// `AdminCopyOwner`), its folder and everything in it are locked again, the group denied
/// (`admin_copy::locked`), so no pool process can hold the copy against the SYSTEM tasks that
/// run it. A first install's `--adopt` locked before there was a group; a re-run locks again
/// all the same, which also mends a deny that went missing. An install with no admin copy has
/// nothing to lock. A record that can't be read or a lock that fails is [`Step::AdminLock`],
/// rolled back; the deny itself isn't, and a deny for a group that is gone opens nothing.
pub(super) fn admin_lock(machine: &mut impl Machine, group: &Sid) -> Result<(), Failed> {
    let failed = |why: String| (Step::AdminLock, format!("the admin copy's lock: {why}"));
    match machine.copy_owner() {
        Ok(Some(_)) => machine
            .lock_admin_copy(group)
            .map_err(|error| failed(cause_of(&error))),
        Ok(None) => Ok(()),
        Err(error) => Err(failed(format!(
            "the record's AdminCopyOwner can't be read ({})",
            cause_of(&error)
        ))),
    }
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

/// WSL's deny (`windows::wsl`): the pool group denied in the launch permission of every WSL
/// class on this machine, elevated, not as the owner, twice: right after the record is written
/// (the group and the record both exist), and again after the marker. Never fatal, like step 9:
/// the launch check refuses a tier B launch while a deny is missing, and the SYSTEM tasks try
/// again (the refresh at the next logon, the WSL deny task at the next installer event). What
/// failed is in the report and the log.
pub(super) fn wsl_deny(machine: &mut impl Machine, group: &Sid) -> Result<WslApplied, String> {
    let applied = machine.wsl_deny(group).map_err(|error| cause_of(&error));
    match &applied {
        Ok(done) if !done.complete() => {
            tracing::warn!(failed = ?done.failed, "WSL's deny wasn't written everywhere");
        }
        Ok(_) => {}
        Err(why) => tracing::warn!(%why, "WSL's deny wasn't written"),
    }
    applied
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
