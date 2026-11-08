//! `--remove` (S15 `PR4j`): the uninstall of what set-up made, in the order the uninstaller's
//! hook relies on, and the exits it reads ([`SetupExit::Removed`] is the only success). Pure
//! apart from its seams ([`Removal`], `seam.rs`; the permission writes through [`AclWriter`]),
//! so the order is tested on every OS against a fake. `windows_removal.rs` is the real machine.
//!
//! Who acts (D-B2, D-B9 as ruled): the user who asked is the token user of the process Windows
//! recorded as the parent ([`Removal::invoker`]), never an argument.
//!
//! - **Per-user** (`--remove`): only the record's owner removes anything. Anyone else gets
//!   [`Removed::NotOwner`] (exit 0), nothing touched, since they have nothing to remove and their
//!   own `sandbox\` may go. A parent that can't be found (it exited; a launch from session 0)
//!   gets [`SetupExit::Failed`], nothing touched, so the hook keeps `sandbox\`.
//! - **Per-machine** (`--remove --per-machine`, added by the per-machine installer only, and
//!   refused unless this process is elevated or LOCAL SYSTEM, [`per_machine_allowed`]): every
//!   pool in the record goes, whoever asked, LOCAL SYSTEM under Intune's silent `/S` included.
//!   When the parent is the owner, the ledger work runs as the owner; otherwise it is limited as
//!   `ledger_work.rs` says, and the record goes even with ledger entries left: they name dead
//!   SIDs, which hold nothing (K10).
//!
//! The owner is acted as only with a token no stronger than a standard user's
//! ([`acting_token`]): the parent's own, or, when the parent is elevated (the per-machine
//! uninstaller always is), a copy lowered to Administrators deny-only and Medium integrity, with
//! no privilege but `SeChangeNotifyPrivilege`. If it can't be lowered, a per-machine removal
//! runs as an administrator's, bounded the same way; a per-user one stops with
//! [`SetupExit::Failed`], nothing touched and the record kept.
//!
//! The order, under the machine-wide set-up lock, waited for at most [`REMOVE_LOCK_WAIT_MS`]
//! (past that [`SetupExit::LockHeld`], nothing touched; the hook's own bound is 120 s, K3).
//! The uninstaller's removal first disables the set-up task and stops its run, which may hold
//! the lock for up to two hours (`quiet_set_up_task`). A
//! first read before the lock that finds no record still tries the lock, without waiting: a
//! set-up holding it may be writing the first record, so that is `LockHeld` too, never 0.
//!
//! 1. the record read again under the lock, and who acts decided again from it;
//! 2. `Removing = 1` written to the record (once; a re-run finds it set);
//! 3. every process running as a pool SID stopped (D-B13);
//! 4. per pool user, in record order: its `LocalLow` denies (planned again from the record's
//!    group, D-B11), its profile, its account with its sign-in screen value. A reparse point on
//!    the way to `LocalLow` (`Removal::in_profile`) is logged, and the deny goes with the
//!    profile, whose delete takes each link in it out itself first;
//!    Then, only once every pool account is gone and a second sweep has stopped every process
//!    still running as a pool SID (one that won't stop ends the run at `Strays`, network on),
//!    the network filters, the owner's read grant
//!    on them and the firewall rules (S15 PR7, `wfp::remove`): `stop_strays` only logs a
//!    process that won't stop, which must not run unfiltered while a user's removal fails. A
//!    firewall failure is not fatal, as at set-up ([`RemoveReport::firewall`]);
//! 5. `DeviceSync`'s deny;
//! 6. the ledger's entries (`ledger_work.rs`), read from the record's `DataDir`, which the owner
//!    can influence, so it is checked as `--data-dir` is and used only as `ledger_work.rs` says;
//! 7. the group, when it is the record's and has no members left;
//! 8. on a per-machine install, the stop task set-up registered for the owner, then the record
//!    key;
//! 9. for the uninstaller only, the admin copy's teardown (`admin_remove.rs`, run through
//!    [`remove_then`]), with the lock still held, so a refresh skips instead of racing it.
//!
//! The first failure stops the run there: [`SetupExit::RemovedPartly`], the record kept with
//! `Removing = 1` and every pool SID (so a later run still trusts the ledger's entries), and the
//! ledger kept. Each step leaves a state the next run starts from, so the hook may stop waiting
//! at any point (it never terminates the exe), and a run that is ended partway only leaves work
//! for the next one. A re-run skips what is gone: an account, a profile or a group that isn't
//! there, a deny already removed. The recovery for a partial removal: reinstall Clauseo, then
//! uninstall it again.

use std::path::PathBuf;

use super::machine::{MachineError, ProfileDir};
use super::outcome::{SetupExit, cause_of};
use super::record_values::CopyOwner;
use crate::windows::pool::{MachineRecord, pool_number};
use crate::windows::shared_writes::{
    plan_shared_write_denies, remove_shared_write_denies, shared_write_targets,
};
use crate::windows::sids::Sid;
use crate::windows::writer::AclWriter;

mod ledger_work;
pub mod owners;
mod seam;
#[cfg(windows)]
mod windows_removal;

pub use ledger_work::LedgerWork;
pub use owners::FolderOwner;
pub use seam::Removal;
#[cfg(windows)]
pub use windows_removal::WindowsRemoval;

/// How long `--remove` waits for another set-up's lock: 60 s, half the hook's 120 s bound.
pub const REMOVE_LOCK_WAIT_MS: u32 = 60_000;

/// What a removal did. Its `Debug` holds counts and causes, never a SID or a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removed {
    /// No record: nothing was set up on this computer.
    NothingSetUp,
    /// A per-user uninstall by someone who isn't the record's owner: nothing touched (D-B9).
    NotOwner,
    /// Everything was removed.
    All(RemoveReport),
}

/// What a finished removal took off.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoveReport {
    /// Profiles deleted.
    pub profiles: usize,
    /// Accounts deleted.
    pub accounts: usize,
    /// The ledger's entries.
    pub ledger: LedgerWork,
    /// Whether the group was deleted (`false`: it was gone already).
    pub group: bool,
    /// Why the firewall rules couldn't be removed, when they couldn't: not fatal, as at set-up,
    /// since the WFP filters (the enforcing layer) went (S15 PR7).
    pub firewall: Option<String>,
}

/// The step a removal stopped at. Variants are appended from here on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveStep {
    /// Taking the set-up lock.
    Lock,
    /// Reading the record.
    ReadRecord,
    /// Finding who asked.
    Owner,
    /// Writing `Removing = 1`.
    Flag,
    /// Stopping the pool users' processes.
    Strays,
    /// A pool user's `LocalLow` denies.
    LocalLow,
    /// A pool user's profile.
    Profile,
    /// A pool user's account.
    Account,
    /// `DeviceSync`'s deny.
    DeviceSync,
    /// The ledger's entries.
    Ledger,
    /// The group.
    Group,
    /// Deleting the record.
    Record,
    /// Removing the network filters and firewall rules (S15 PR7).
    Network,
    /// A per-machine install's stop task, deleted before the record.
    StopTask,
}

impl std::fmt::Display for RemoveStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            RemoveStep::Lock => "taking the set-up lock",
            RemoveStep::ReadRecord => "reading the machine's record",
            RemoveStep::Owner => "finding who asked",
            RemoveStep::Flag => "marking the removal as started",
            RemoveStep::Strays => "stopping the sandbox users' processes",
            RemoveStep::LocalLow => "opening a sandbox user's LocalLow again",
            RemoveStep::Profile => "deleting a sandbox user's profile",
            RemoveStep::Account => "deleting a sandbox user's account",
            RemoveStep::DeviceSync => "opening DeviceSync again",
            RemoveStep::Ledger => "taking the ledger's entries back",
            RemoveStep::Group => "deleting the sandbox group",
            RemoveStep::Record => "deleting the machine's record",
            RemoveStep::Network => "removing the network filters",
            RemoveStep::StopTask => "deleting the sandbox's stop task",
        })
    }
}

/// Why a removal stopped, and the exit the setup exe gives for it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("sandbox removal stopped {step}: {cause}")]
pub struct RemoveFailure {
    /// Where.
    pub step: RemoveStep,
    /// Why, naming accounts by name, never by SID.
    pub cause: String,
    /// [`SetupExit::LockHeld`], [`SetupExit::Failed`] (nothing touched) or
    /// [`SetupExit::RemovedPartly`].
    pub exit: SetupExit,
}

/// Who the removal acts as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Acting {
    /// The record's owner asked: the owner's files are worked on as the owner.
    Owner,
    /// A per-machine removal someone else asked for: no impersonation.
    Admin,
}

/// What the record and the asker decide.
enum Decision {
    Stop(Removed),
    Act(Acting),
}

/// A step's failure inside the run: where, and why.
type Failed = (RemoveStep, String);

/// Runs the removal as the module says. `per_machine` is `--per-machine`.
pub fn remove(
    machine: &mut impl Removal,
    acl: &mut impl AclWriter,
    per_machine: bool,
) -> Result<Removed, RemoveFailure> {
    remove_then(machine, acl, per_machine, false, &mut |_| {})
}

/// [`remove`], then `while_locked` with the set-up lock still held, given what was removed: the
/// uninstaller's admin-copy teardown (`exe::run_remove`), so a refresh, which takes the same
/// lock without waiting, skips rather than race the staging folder's removal. `while_locked`
/// runs on every success but a per-user one by someone else decided before the lock
/// ([`Removed::NotOwner`]): the pool has an owner then, so the copy isn't this removal's.
/// `uninstall` is `--uninstall`: the set-up task is disabled and stopped before the lock is
/// waited for ([`quiet_set_up_task`]).
pub fn remove_then(
    machine: &mut impl Removal,
    acl: &mut impl AclWriter,
    per_machine: bool,
    uninstall: bool,
    while_locked: &mut dyn FnMut(&Removed),
) -> Result<Removed, RemoveFailure> {
    // Read once before the lock, so a machine with nothing set up, or a per-user uninstall by
    // someone else, ends at once and never waits for a set-up's lock.
    let before = read(machine)?;
    if before.is_none() {
        // No record, unless a set-up holding the lock is about to write it.
        lock(machine, 0)?;
        let _locked = Locked(machine);
        while_locked(&Removed::NothingSetUp);
        return Ok(Removed::NothingSetUp);
    }
    if let Decision::Stop(removed) = decide(machine, before.as_ref(), per_machine)? {
        return Ok(removed);
    }
    if uninstall {
        quiet_set_up_task(machine);
    }
    lock(machine, REMOVE_LOCK_WAIT_MS)?;
    let locked = Locked(machine);
    let machine = &mut *locked.0;
    let record = read(machine)?;
    let acting = match decide(machine, record.as_ref(), per_machine)? {
        Decision::Stop(removed) => {
            while_locked(&removed);
            return Ok(removed);
        }
        Decision::Act(acting) => acting,
    };
    let Some(record) = record else {
        while_locked(&Removed::NothingSetUp);
        return Ok(Removed::NothingSetUp);
    };
    let removed = steps(machine, acl, &record, acting)
        .map(Removed::All)
        .map_err(|(step, cause)| RemoveFailure {
            step,
            cause,
            exit: SetupExit::RemovedPartly,
        })?;
    while_locked(&removed);
    Ok(removed)
}

/// Before the uninstaller's removal waits for the lock: the set-up task disabled, so no set-up
/// starts from here on, then its running instance stopped, whose set-up gives the lock back as
/// it ends (a run may last two hours, past the lock's wait). Each is idempotent, and one that
/// fails is logged and left to the lock's wait, as before; the teardown deletes the task.
fn quiet_set_up_task(machine: &mut impl Removal) {
    if let Err(error) = machine.disable_set_up_task() {
        let cause = cause_of(&error);
        tracing::warn!(%cause, "the set-up task couldn't be disabled");
    }
    if let Err(error) = machine.stop_set_up_task() {
        let cause = cause_of(&error);
        tracing::warn!(%cause, "the set-up task's run couldn't be stopped");
    }
}

/// The set-up lock, waited for up to `wait_ms`; held still is [`SetupExit::LockHeld`].
fn lock(machine: &mut impl Removal, wait_ms: u32) -> Result<(), RemoveFailure> {
    machine.lock_within(wait_ms).map_err(|error| {
        let exit = if error == MachineError::LockHeld {
            SetupExit::LockHeld
        } else {
            SetupExit::Failed
        };
        RemoveFailure {
            step: RemoveStep::Lock,
            cause: cause_of(&error),
            exit,
        }
    })
}

/// Whether `--per-machine` may run in this process: elevated, or LOCAL SYSTEM. Anything else
/// (a standard user running the exe with UAC off, say) exits `Usage` with nothing touched.
pub fn per_machine_allowed(elevated: bool, system: bool) -> bool {
    elevated || system
}

/// The token to act as the owner with: `token` itself when `elevated` says it isn't elevated;
/// when it is, the copy `lower` makes; `None` when either fails, so the owner's removal runs as
/// an administrator's, bounded as `ledger_work.rs` says. Never the elevated token itself.
pub fn acting_token<T, E: std::fmt::Display>(
    token: T,
    elevated: impl FnOnce(&T) -> Result<bool, E>,
    lower: impl FnOnce(&T) -> Result<T, E>,
) -> Option<T> {
    match elevated(&token) {
        Ok(false) => Some(token),
        Ok(true) => lower(&token)
            .inspect_err(|error| tracing::warn!(%error, "the owner's token couldn't be lowered"))
            .ok(),
        Err(error) => {
            tracing::warn!(%error, "the owner's token couldn't be checked");
            None
        }
    }
}

/// The record, or a failure with nothing touched.
fn read(machine: &mut impl Removal) -> Result<Option<MachineRecord>, RemoveFailure> {
    machine.read_record().map_err(|error| RemoveFailure {
        step: RemoveStep::ReadRecord,
        cause: cause_of(&error),
        exit: SetupExit::Failed,
    })
}

/// Whether to stop, and who acts otherwise, as the module says.
fn decide(
    machine: &impl Removal,
    record: Option<&MachineRecord>,
    per_machine: bool,
) -> Result<Decision, RemoveFailure> {
    let Some(record) = record else {
        return Ok(Decision::Stop(Removed::NothingSetUp));
    };
    match machine.invoker() {
        Some(invoker) if *invoker == record.owner && machine.can_act_as_owner() => {
            Ok(Decision::Act(Acting::Owner))
        }
        // The owner asked, but no token weaker than an administrator's can act as them. A
        // per-machine run takes the administrator's bounded path; a per-user one stops, since
        // that path would delete the record with ledger entries left.
        Some(invoker) if *invoker == record.owner && per_machine => {
            Ok(Decision::Act(Acting::Admin))
        }
        Some(invoker) if *invoker == record.owner => Err(RemoveFailure {
            step: RemoveStep::Owner,
            cause: "the owner's token couldn't be lowered to a standard user's, so nothing was \
                    removed"
                .to_owned(),
            exit: SetupExit::Failed,
        }),
        _ if per_machine => Ok(Decision::Act(Acting::Admin)),
        Some(_) => Ok(Decision::Stop(Removed::NotOwner)),
        None => Err(RemoveFailure {
            step: RemoveStep::Owner,
            cause: "the user who asked can't be found, so nothing was removed".to_owned(),
            exit: SetupExit::Failed,
        }),
    }
}

/// Steps 2 to 8.
fn steps(
    machine: &mut impl Removal,
    acl: &mut impl AclWriter,
    record: &MachineRecord,
    acting: Acting,
) -> Result<RemoveReport, Failed> {
    let mut report = RemoveReport::default();
    if !record.removing {
        let flagged = MachineRecord {
            removing: true,
            ..record.clone()
        };
        machine
            .write_record(&flagged)
            .map_err(|e| (RemoveStep::Flag, cause_of(&e)))?;
    }
    // A process left running here is logged; deleting its profile or account fails if it matters.
    machine
        .stop_strays(&record.pool, &record.owner)
        .map_err(|e| (RemoveStep::Strays, cause_of(&e)))?;
    for (at, sid) in record.pool.iter().enumerate() {
        remove_user(machine, acl, record, at, sid, &mut report)?;
    }
    // Anything still running as a pool SID (one that started since the first sweep, or one the
    // first couldn't stop) is stopped again, elevated, before the network goes. One that won't
    // stop keeps the network on: the run ends here, RemovedPartly, and a re-run retries.
    let left = machine
        .stop_strays(&record.pool, &record.owner)
        .map_err(|e| (RemoveStep::Strays, cause_of(&e)))?;
    if left > 0 {
        return Err((
            RemoveStep::Strays,
            format!(
                "{left} process(es) of the sandbox users wouldn't stop, so the network \
                 protection stays on; restart the computer and uninstall again"
            ),
        ));
    }
    report.firewall = machine
        .remove_network(&record.owner)
        .map_err(|e| (RemoveStep::Network, cause_of(&e)))?;
    let program_data = machine
        .program_data()
        .map_err(|e| (RemoveStep::DeviceSync, cause_of(&e)))?;
    let device_sync = shared_write_targets(&program_data, &[]);
    remove_denies(acl, &record.group, &device_sync).map_err(|cause| {
        (
            RemoveStep::DeviceSync,
            format!("DeviceSync's deny: {cause}"),
        )
    })?;
    report.ledger = ledger_work::run(machine, acl, record, acting)?;
    report.group = remove_group(machine, record)?;
    remove_stop_task(machine)?;
    machine
        .delete_record()
        .map_err(|e| (RemoveStep::Record, cause_of(&e)))?;
    Ok(report)
}

/// One pool user, the `at`th in the record: its `LocalLow` denies, its profile, its account.
/// Causes name it by its account's name, or by its place in the record once the account is
/// gone; never by its SID.
fn remove_user(
    machine: &mut impl Removal,
    acl: &mut impl AclWriter,
    record: &MachineRecord,
    at: usize,
    sid: &Sid,
    report: &mut RemoveReport,
) -> Result<(), Failed> {
    let account = |e: MachineError| (RemoveStep::Account, cause_of(&e));
    let name = machine.account_name(sid).map_err(account)?;
    let label = name
        .clone()
        .unwrap_or_else(|| format!("sandbox user {}", at + 1));
    if name
        .as_deref()
        .is_some_and(|name| pool_number(name).is_none())
    {
        return Err((
            RemoveStep::Account,
            format!("{label} isn't named as a sandbox user, so it is left alone"),
        ));
    }
    let profile = machine
        .profile_of(sid)
        .map_err(|e| (RemoveStep::Profile, cause_of(&e)))?;
    if let Some(ProfileDir { path, temporary }) = &profile {
        // A temporary profile's `LocalLow` is not the user's own, and set-up put no deny there.
        if !temporary {
            let local_low = path.join("AppData").join("LocalLow");
            let deny_failed = |cause: String| {
                (
                    RemoveStep::LocalLow,
                    format!("{label}'s LocalLow deny: {cause}"),
                )
            };
            match machine.in_profile(path, &local_low) {
                // A reparse point on the way: the real folder, and its deny, go with the
                // profile, whose delete takes the link out itself first.
                Err(MachineError::ProfileChanged(part)) => tracing::warn!(
                    user = %label,
                    %part,
                    "a folder in a sandbox user's profile isn't a plain folder; its LocalLow deny \
                     goes with the profile"
                ),
                Err(error) => return Err(deny_failed(cause_of(&error))),
                Ok(_) => remove_denies(acl, &record.group, &[local_low]).map_err(deny_failed)?,
            }
        }
        machine
            .delete_profile(sid, &record.owner)
            .map_err(|e| (RemoveStep::Profile, format!("{label}: {}", cause_of(&e))))?;
        report.profiles += 1;
    }
    if let Some(name) = &name {
        machine
            .delete_user(name)
            .map_err(|e| (RemoveStep::Account, format!("{label}: {}", cause_of(&e))))?;
        report.accounts += 1;
    }
    Ok(())
}

/// The shared-write denies on `targets`, planned again from `group` (D-B11), removed and
/// walked down. A target that is gone is skipped.
fn remove_denies(acl: &mut impl AclWriter, group: &Sid, targets: &[PathBuf]) -> Result<(), String> {
    let grants = plan_shared_write_denies(group, targets).map_err(|e| e.to_string())?;
    remove_shared_write_denies(acl, &grants).map_err(|failure| failure.to_string())
}

/// The group, when it is the record's and empty; whether it was deleted. Gone already is
/// `false`; another group under the name, or one with members, stops the run.
fn remove_group(machine: &mut impl Removal, record: &MachineRecord) -> Result<bool, Failed> {
    let failed = |e: MachineError| (RemoveStep::Group, cause_of(&e));
    let Some(group) = machine.group().map_err(failed)? else {
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
        0 => machine.delete_group(&group).map_err(failed).map(|()| true),
        members => Err((
            RemoveStep::Group,
            format!("the group ClauseoSandboxUsers still has {members} members"),
        )),
    }
}

/// Step 8, before the record goes: on a per-machine install (`AdminCopyOwner` is `machine`),
/// the task `\Clauseo\SandboxStop` set-up registered for the record's owner is deleted, so
/// the owner keeps no start right once their pool is gone. A per-user install's task stays
/// (`--adopt` registered it; `--remove --uninstall` deletes it with the admin copy).
fn remove_stop_task(machine: &mut impl Removal) -> Result<(), Failed> {
    let failed = |e: MachineError| (RemoveStep::StopTask, cause_of(&e));
    match machine.copy_owner().map_err(failed)? {
        Some(CopyOwner::Machine) => machine.unregister_stop_task().map_err(failed),
        Some(CopyOwner::User(_)) | None => Ok(()),
    }
}

/// The machine while the removal holds its lock: dropping it gives the lock back.
struct Locked<'m, M: Removal>(&'m mut M);

impl<M: Removal> Drop for Locked<'_, M> {
    fn drop(&mut self) {
        self.0.unlock();
    }
}
