//! A set-up run, steps 1 to 11 after the lock: the owner check, the preflight, the plan for the
//! pool (D-B5, D-B6), then the changes, each failure handed to the rollback (`rollback.rs`).

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use super::journal_plan::{JournalAction, journal_plan};
use super::machine::{LogonRight, Machine, Password, ServiceStart, owned};
use super::outcome::{
    JournalOutcome, LOGON_REFUSED, SECLOGON_DISABLED, SetupExit, SetupFailure, SetupReport, Step,
    cause_of,
};
use super::rollback::{Made, Run, roll_back};
use super::{POOL_START, SetupRequest};
use crate::windows::apply::{Applied, fence_registered};
use crate::windows::marker::{Marker, MarkerState, SETUP_VERSION};
use crate::windows::pool::{MachineRecord, POOL_CAP, PoolUser, pool_name, pool_number};
use crate::windows::shared_writes::{
    SharedWrites, apply_shared_write_denies, plan_shared_write_denies, shared_write_targets,
};
use crate::windows::sids::Sid;
use crate::windows::writer::{AclWriter, ApplyFailure};

/// A step's failure inside the run: where, and why.
type Failed = (Step, String);

/// Who the pool is before the changes, and what the run will make.
#[derive(Debug, Default)]
struct Plan {
    /// The recorded users whose accounts exist, in record order.
    kept: Vec<PoolUser>,
    /// The record's SIDs that stay in it, in record order: the kept users' and the dead ones a
    /// ledger entry names.
    base: Vec<Sid>,
    /// Recorded SIDs whose accounts are gone and which a ledger entry names.
    kept_dead: Vec<Sid>,
    /// Recorded SIDs whose accounts are gone and which no ledger entry names.
    dropped: Vec<Sid>,
    /// How many users to make.
    make: usize,
    /// A re-run: the kept users' passwords are reset (D-B6).
    reset: bool,
}

/// What the users' steps hand on: each live user's profile and the passwords set this run.
#[derive(Default)]
struct Users {
    /// Every live pool user, kept then made, with its profile folder.
    profiles: Vec<(PoolUser, PathBuf)>,
    /// The passwords this run set.
    secrets: Vec<(Sid, Password)>,
    /// The users whose passwords this run reset.
    reset: Vec<PoolUser>,
    /// Pool names skipped for an account outside the record (D-B5).
    skipped: Vec<(String, Sid)>,
}

/// The run after the lock: everything [`set_up`](super::set_up) does inside it.
pub(super) fn run(
    machine: &mut impl Machine,
    acl: &mut impl AclWriter,
    req: &SetupRequest,
) -> Result<SetupReport, SetupFailure> {
    let before = machine
        .read_record()
        .map_err(|error| SetupFailure::early(Step::ReadRecord, cause_of(&error)))?;
    check_owner(before.as_ref(), &req.owner)?;
    preflight(machine)?;
    let plan = plan_pool(machine, before.as_ref(), req)?;
    let mut run = Run::new(before);
    match change(machine, acl, req, &plan, &mut run) {
        Ok(report) => Ok(report),
        Err((step, cause)) => {
            let left = roll_back(machine, acl, req, &run);
            let rolled_back = left.is_empty();
            let cause = if rolled_back {
                cause
            } else {
                format!("{cause}; left behind: {}", left.join("; "))
            };
            Err(SetupFailure {
                step,
                cause,
                rolled_back,
                exit: SetupExit::Failed,
            })
        }
    }
}

/// Step 1's check: a record owned by another user refuses with that user's SID (D5); one that
/// names the owner as a pool user or the group refuses too, so no step ever touches the owner's
/// own account (S15 `PR4b` review).
fn check_owner(record: Option<&MachineRecord>, owner: &Sid) -> Result<(), SetupFailure> {
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
fn preflight(machine: &mut impl Machine) -> Result<(), SetupFailure> {
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

/// The plan: which recorded users stay, which dead SIDs stay or go, how many to make.
///
/// A recorded SID whose account exists stays, and must be a pool name, or set-up refuses
/// rather than touch another account. A dead SID stays while a ledger entry names it (or the
/// ledger can't be read), else it goes. Each dead SID the marker lists was a live pool user at
/// the last set-up and is made again under a new SID; with no current marker, every dead SID is.
/// A set-up makes enough for [`POOL_START`] users at least; an extend makes `add` more; neither
/// goes past [`POOL_CAP`] SIDs in the record, dead ones kept for the ledger included. A re-run
/// with users to reset refuses while a session is open.
fn plan_pool(
    machine: &mut impl Machine,
    before: Option<&MachineRecord>,
    req: &SetupRequest,
) -> Result<Plan, SetupFailure> {
    let pool_failure = |cause: String| SetupFailure::early(Step::Pool, cause);
    let mut plan = Plan::default();
    let mut dead = Vec::new();
    for sid in before
        .map(|record| record.pool.as_slice())
        .unwrap_or_default()
    {
        match machine.account_name(sid) {
            Ok(Some(name)) if pool_number(&name).is_some() => {
                plan.kept.push(PoolUser {
                    name,
                    sid: sid.clone(),
                });
            }
            Ok(Some(name)) => {
                return Err(pool_failure(format!(
                    "the machine's record lists {sid}, which is the account {name}, not a \
                     sandbox user"
                )));
            }
            Ok(None) => dead.push(sid.clone()),
            Err(error) => return Err(pool_failure(cause_of(&error))),
        }
    }
    let mut missing = 0;
    if let (Some(record), false) = (before, dead.is_empty()) {
        let marker = machine.as_owner(Machine::read_marker);
        missing = match marker.unwrap_or_else(|error| MarkerState::Unreadable(error.to_string())) {
            MarkerState::Current(marker) => {
                dead.iter().filter(|sid| marker.pool.contains(sid)).count()
            }
            MarkerState::Missing
            | MarkerState::Older { .. }
            | MarkerState::OtherUser { .. }
            | MarkerState::Newer { .. }
            | MarkerState::Unreadable(_) => dead.len(),
        };
        let mut trusted = record.pool.clone();
        trusted.push(record.group.clone());
        let ledger = machine.as_owner(|machine| machine.ledger(&trusted));
        let ledger = ledger.unwrap_or_else(|error| Err(io::Error::other(error)));
        if let Err(error) = &ledger {
            tracing::warn!(%error, "the ledger can't be read; set-up keeps every dead pool SID");
        }
        for sid in dead {
            match &ledger {
                Ok(ledger) if !ledger.names_sid(&sid) => plan.dropped.push(sid),
                _ => plan.kept_dead.push(sid),
            }
        }
        plan.base = record
            .pool
            .iter()
            .filter(|sid| !plan.dropped.contains(sid))
            .cloned()
            .collect();
    } else {
        plan.base = plan.kept.iter().map(|user| user.sid.clone()).collect();
    }
    let kept = plan.kept.len();
    let add = usize::from(req.add);
    let wanted = if add == 0 {
        (kept + missing).max(POOL_START)
    } else {
        kept + missing + add
    };
    // Dead SIDs kept in the record count too, so `PoolSids` never holds more than the cap.
    let room = usize::from(POOL_CAP).saturating_sub(plan.base.len());
    plan.make = wanted
        .min(usize::from(POOL_CAP))
        .saturating_sub(kept)
        .min(room);
    if add > 0 && plan.make == 0 {
        return Err(pool_failure(format!(
            "the pool's record already holds {} sandbox users, the most there can be",
            plan.base.len()
        )));
    }
    plan.reset = add == 0 && kept > 0;
    if plan.reset {
        match owned(machine, Machine::sessions_open) {
            Ok(false) => {}
            Ok(true) => {
                return Err(SetupFailure::early(
                    Step::Sessions,
                    "a matter is still open in Clauseo; close every matter, then run set-up \
                     again",
                ));
            }
            Err(error) => return Err(SetupFailure::early(Step::Sessions, cause_of(&error))),
        }
    }
    Ok(plan)
}

/// Steps 3 to 11: everything that changes the machine. The first failure returns at once; the
/// caller rolls back what `run` lists.
fn change(
    machine: &mut impl Machine,
    acl: &mut impl AclWriter,
    req: &SetupRequest,
    plan: &Plan,
    run: &mut Run,
) -> Result<SetupReport, Failed> {
    let group = group(machine, run)?;
    let mut users = Users::default();
    run.kept = plan.kept.iter().map(|user| user.sid.clone()).collect();
    for user in &plan.kept {
        keep_user(machine, plan, run, user, &group, &mut users)?;
    }
    make_users(machine, req, plan, run, &group, &mut users)?;
    let record = record_of(req, &group, plan, run);
    if run.recorded() != Some(&record) {
        write_record(machine, &record).map_err(|cause| (Step::WriteRecord, cause))?;
        run.recorded = Some(record);
    }
    let shared = shared_writes(machine, acl, &group, &users.profiles, run)?;
    let live: Vec<Sid> = users.profiles.iter().map(|(u, _)| u.sid.clone()).collect();
    let fenced = fence(machine, acl, req, run, &group, &live)?;
    let journals = journals(machine, req);
    owned(machine, |machine| {
        machine.protect_for_owner(&users.secrets, &live)
    })
    .map_err(|error| (Step::Secrets, cause_of(&error)))?;
    let marker = Marker {
        version: SETUP_VERSION,
        user_sid: req.owner.clone(),
        group_sid: group.clone(),
        pool: live,
        set_up_at: machine.now(),
    };
    owned(machine, |machine| machine.write_marker(&marker))
        .map_err(|error| (Step::Marker, cause_of(&error)))?;
    Ok(SetupReport {
        pool: users.profiles.into_iter().map(|(user, _)| user).collect(),
        made: run.made.iter().map(|made| made.user.clone()).collect(),
        reset: users.reset,
        group,
        group_made: run.group_made,
        dropped: plan.dropped.clone(),
        kept_dead: plan.kept_dead.clone(),
        skipped_names: users.skipped,
        shared,
        fenced,
        journals,
    })
}

/// Step 3: the group, adopted only when its SID is the record's, made when missing. A group by
/// that name that isn't the record's (or with no record) refuses: it is someone else's.
fn group(machine: &mut impl Machine, run: &mut Run) -> Result<Sid, Failed> {
    let failed = |cause: String| (Step::Group, cause);
    let found = machine.group().map_err(|error| failed(cause_of(&error)))?;
    let recorded = run.before.as_ref().map(|record| &record.group);
    let sid = match found {
        Some(sid) if Some(&sid) == recorded => sid,
        Some(sid) => {
            return Err(failed(format!(
                "a group named ClauseoSandboxUsers exists ({sid}) that set-up didn't record"
            )));
        }
        None => {
            let sid = machine
                .create_group()
                .map_err(|error| failed(cause_of(&error)))?;
            run.group_made = true;
            sid
        }
    };
    run.group = Some(sid.clone());
    Ok(sid)
}

/// A recorded user that exists. A re-run resets its password, puts it back in its groups,
/// hides it and logs it on (which makes its profile again if it was deleted). An extend leaves
/// it as it is, but puts it in a group this run made, and reads its profile.
fn keep_user(
    machine: &mut impl Machine,
    plan: &Plan,
    run: &mut Run,
    user: &PoolUser,
    group: &Sid,
    users: &mut Users,
) -> Result<(), Failed> {
    let name = user.name.as_str();
    if !plan.reset {
        if run.group_made {
            machine
                .add_to_group(name, group)
                .map_err(|error| (Step::AddToGroup, cause_of(&error)))?;
        }
        let profile = machine
            .profile_of(&user.sid)
            .map_err(|error| (Step::Profile, cause_of(&error)))?;
        let Some(profile) = profile.filter(|profile| !profile.temporary) else {
            return Err((
                Step::Profile,
                format!("{name} has no profile of its own; run set-up again without adding users"),
            ));
        };
        users.profiles.push((user.clone(), profile.path));
        return Ok(());
    }
    let password = machine.new_password();
    machine
        .set_password(name, &password)
        .map_err(|error| (Step::ResetPassword, cause_of(&error)))?;
    run.reset.push((user.sid.clone(), password.clone()));
    users.reset.push(user.clone());
    machine
        .add_to_group(name, group)
        .map_err(|error| (Step::AddToGroup, cause_of(&error)))?;
    machine
        .hide(name)
        .map_err(|error| (Step::Hide, cause_of(&error)))?;
    let path = log_on(machine, name, &password)?;
    users.profiles.push((user.clone(), path));
    users.secrets.push((user.sid.clone(), password));
    Ok(())
}

/// Steps 4 to 6 for each new user: a free name, a password, the account, its groups, hidden;
/// the record with its SID (a failed write rolls the user back, as every user this run made);
/// then one logon for its profile.
fn make_users(
    machine: &mut impl Machine,
    req: &SetupRequest,
    plan: &Plan,
    run: &mut Run,
    group: &Sid,
    users: &mut Users,
) -> Result<(), Failed> {
    let taken: BTreeSet<&str> = plan.kept.iter().map(|user| user.name.as_str()).collect();
    let mut next = 1;
    for _ in 0..plan.make {
        let name = free_name(machine, &taken, &mut next, &mut users.skipped)?;
        let password = machine.new_password();
        let sid = machine
            .create_user(&name, &password)
            .map_err(|error| (Step::CreateUser, cause_of(&error)))?;
        let user = PoolUser { name, sid };
        run.made.push(Made {
            user: user.clone(),
            logged_on: false,
            profile: None,
        });
        let name = user.name.as_str();
        machine
            .add_to_group(name, group)
            .map_err(|error| (Step::AddToGroup, cause_of(&error)))?;
        machine
            .hide(name)
            .map_err(|error| (Step::Hide, cause_of(&error)))?;
        let record = record_of(req, group, plan, run);
        write_record(machine, &record).map_err(|cause| (Step::WriteRecord, cause))?;
        run.recorded = Some(record);
        if let Some(made) = run.made.last_mut() {
            made.logged_on = true;
        }
        let path = log_on(machine, name, &password)?;
        if let Some(made) = run.made.last_mut() {
            made.profile = Some(path.clone());
        }
        users.profiles.push((user.clone(), path));
        users.secrets.push((user.sid, password));
    }
    Ok(())
}

/// The next pool name from `next` on that no kept user has and no account outside the record
/// holds: such an account is never adopted, but skipped and listed (D-B5).
fn free_name(
    machine: &mut impl Machine,
    taken: &BTreeSet<&str>,
    next: &mut u8,
    skipped: &mut Vec<(String, Sid)>,
) -> Result<String, Failed> {
    while let Some(name) = pool_name(*next) {
        *next += 1;
        if taken.contains(name.as_str()) {
            continue;
        }
        match machine.account_sid(&name) {
            Ok(None) => return Ok(name),
            Ok(Some(sid)) => {
                tracing::info!(%name, %sid, "an account outside the record has this name; skipped");
                skipped.push((name, sid));
            }
            Err(error) => return Err((Step::CreateUser, cause_of(&error))),
        }
    }
    Err((
        Step::CreateUser,
        format!("no sandbox user name is free (ClauseoSb01 to ClauseoSb{POOL_CAP})"),
    ))
}

/// Step 6: one logon with the profile loaded. A temporary profile is a failure: its `LocalLow`
/// isn't the user's own (K5).
fn log_on(machine: &mut impl Machine, name: &str, password: &Password) -> Result<PathBuf, Failed> {
    let profile = machine
        .log_on_once(name, password)
        .map_err(|error| (Step::LogOn, cause_of(&error)))?;
    if profile.temporary {
        return Err((
            Step::Profile,
            format!("Windows gave {name} a temporary profile, so its folders can't be closed"),
        ));
    }
    Ok(profile.path)
}

/// The record as this run has it now: the base, then every user made so far.
fn record_of(req: &SetupRequest, group: &Sid, plan: &Plan, run: &Run) -> MachineRecord {
    let mut pool = plan.base.clone();
    pool.extend(run.made.iter().map(|made| made.user.sid.clone()));
    MachineRecord {
        owner: req.owner.clone(),
        group: group.clone(),
        pool,
        data_dir: req.data_dir.clone(),
        removing: false,
    }
}

/// Writes `record`, refusing one whose pool or group is the owner's SID: the owner's app would
/// refuse to start as a pool user (S15 `PR4b` review). Every record write goes through here.
pub(super) fn write_record(
    machine: &mut impl Machine,
    record: &MachineRecord,
) -> Result<(), String> {
    if record.pool.contains(&record.owner) || record.group == record.owner {
        return Err(format!(
            "refused to record the owner, {}, as a sandbox user or group",
            record.owner
        ));
    }
    machine
        .write_record(record)
        .map_err(|error| cause_of(&error))
}

/// Step 7: `DeviceSync` and every live user's `LocalLow` denied to the group. Any failed target
/// fails the set-up; what was written stays (D12), except what the rollback takes off with the
/// users this run made.
fn shared_writes(
    machine: &mut impl Machine,
    acl: &mut impl AclWriter,
    group: &Sid,
    profiles: &[(PoolUser, PathBuf)],
    run: &mut Run,
) -> Result<SharedWrites, Failed> {
    let failed = |cause: String| (Step::SharedWrites, cause);
    let program_data = machine
        .program_data()
        .map_err(|error| failed(cause_of(&error)))?;
    let folders: Vec<PathBuf> = profiles.iter().map(|(_, path)| path.clone()).collect();
    let targets = shared_write_targets(&program_data, &folders);
    let grants =
        plan_shared_write_denies(group, &targets).map_err(|error| failed(error.to_string()))?;
    let live: Vec<Sid> = profiles.iter().map(|(user, _)| user.sid.clone()).collect();
    let done = apply_shared_write_denies(acl, &grants, &live);
    run.added.clone_from(&done.added);
    if done.failed.is_empty() {
        Ok(done)
    } else {
        Err(failed(failed_targets(&done.failed)))
    }
}

/// Each failed target and why, for the cause.
fn failed_targets(failed: &[(PathBuf, ApplyFailure)]) -> String {
    let each: Vec<String> = failed
        .iter()
        .map(|(path, failure)| format!("{}: {failure}", path.display()))
        .collect();
    format!("a shared folder couldn't be closed ({})", each.join("; "))
}

/// Step 8: when this run made users and matters are registered (an extend, or a re-run that made
/// a user again), every registered matter is fenced against the grown pool through the owner's
/// ledger, which the rollback reads after. The whole step runs as the owner: the ledger is the
/// owner's file, and the matters' folders are the owner's, so the elevated process writes there
/// only what the owner could (K1).
fn fence(
    machine: &mut impl Machine,
    acl: &mut impl AclWriter,
    req: &SetupRequest,
    run: &mut Run,
    group: &Sid,
    live: &[Sid],
) -> Result<Option<Applied>, Failed> {
    if run.made.is_empty() || req.matters.is_empty() {
        return Ok(None);
    }
    let mut trusted: Vec<Sid> = run.recorded().map(|r| r.pool.clone()).unwrap_or_default();
    trusted.push(group.clone());
    let failed = |cause: String| (Step::Fence, cause);
    let (ledger, fenced) = machine
        .as_owner(|machine| {
            let mut ledger = machine.ledger(&trusted)?;
            let around = machine.surroundings();
            let fenced = fence_registered(&mut ledger, &req.matters, live, acl, around);
            Ok::<_, io::Error>((ledger, fenced))
        })
        .map_err(|error| failed(cause_of(&error)))?
        .map_err(|error| failed(format!("the permission ledger: {error}")))?;
    run.ledger = Some(ledger);
    fenced.map(Some).map_err(|error| failed(error.to_string()))
}

/// Step 9: each chosen volume's change journal made [`super::JOURNAL_MAX`] when smaller
/// ([`journal_plan`], D-B12).
/// Failures are listed, never fatal: they cost only full walks later.
fn journals(
    machine: &mut impl Machine,
    req: &SetupRequest,
) -> Result<Vec<(PathBuf, JournalOutcome)>, String> {
    let roots: Vec<&Path> = req
        .matters
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
