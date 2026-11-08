//! A failed set-up's rollback (D-B7). Users this run made come off in the uninstall order: their
//! `LocalLow` deny, their profile, their account. Then the group goes, only if this run made it
//! and it is empty. Then the record is written to what really remains: every SID it held before,
//! each made user still there (or gone but named by a ledger entry), and a group this run made
//! that is still there, so no account or group is left that the record doesn't name. Every other
//! deny stays (D12). Nothing that existed before the run is removed.
//!
//! A re-run's new passwords for the recorded users can't be undone, so the rollback writes them
//! to `secrets.bin` (keeping the blobs of the users not reset), and the pool still works. The
//! denies stay on purpose: a deny naming a group or user that is gone does no harm.

use std::path::{Path, PathBuf};

use super::SetupRequest;
use super::create::write_record;
use super::machine::{Machine, Password, owned};
use super::outcome::cause_of;
use crate::windows::acl_plan::Grant;
use crate::windows::ledger::Ledger;
use crate::windows::pool::{MachineRecord, PoolUser};
use crate::windows::shared_writes::remove_shared_write_denies;
use crate::windows::sids::Sid;
use crate::windows::writer::AclWriter;

/// A user this run made.
#[derive(Debug)]
pub(super) struct Made {
    /// Its name and SID.
    pub(super) user: PoolUser,
    /// Its logon was tried, so a profile may exist.
    pub(super) logged_on: bool,
    /// Its profile folder, once the logon gave one.
    pub(super) profile: Option<PathBuf>,
}

/// What a run has changed so far: what its rollback reads. No `Debug`: it holds passwords.
pub(super) struct Run {
    /// The record before the run; `None` when there was none.
    pub(super) before: Option<MachineRecord>,
    /// The group, once step 3 found or made it.
    pub(super) group: Option<Sid>,
    /// This run made the group.
    pub(super) group_made: bool,
    /// The users this run made, in order.
    pub(super) made: Vec<Made>,
    /// The record this run last wrote; `None` until it writes one.
    pub(super) recorded: Option<MachineRecord>,
    /// The shared-write grants this run added.
    pub(super) added: Vec<Grant>,
    /// The owner's ledger, once step 8 fenced through it.
    pub(super) ledger: Option<Ledger>,
    /// The recorded users whose accounts exist, kept by this run.
    pub(super) kept: Vec<Sid>,
    /// The new passwords this run gave kept users (a re-run), as each was set.
    pub(super) reset: Vec<(Sid, Password)>,
}

impl Run {
    /// A run that has changed nothing, over the record `before`.
    pub(super) fn new(before: Option<MachineRecord>) -> Run {
        Run {
            before,
            group: None,
            group_made: false,
            made: Vec::new(),
            recorded: None,
            added: Vec::new(),
            ledger: None,
            kept: Vec::new(),
            reset: Vec::new(),
        }
    }

    /// The record as the machine has it now: the last one this run wrote, else the one before.
    pub(super) fn recorded(&self) -> Option<&MachineRecord> {
        self.recorded.as_ref().or(self.before.as_ref())
    }

    /// Whether the ledger, as step 8 last read it, names `sid`.
    fn ledger_names(&self, sid: &Sid) -> bool {
        self.ledger
            .as_ref()
            .is_some_and(|ledger| ledger.names_sid(sid))
    }
}

/// Undoes what `run` made, as the module says. Returns what is left behind, each in words; empty
/// when the rollback finished.
pub(super) fn roll_back(
    machine: &mut impl Machine,
    acl: &mut impl AclWriter,
    req: &SetupRequest,
    run: &Run,
) -> Vec<String> {
    let mut left = Vec::new();
    // Made SIDs the record keeps: an account that is still there, or one gone whose SID a
    // ledger entry names (its entries stay trusted for removal).
    let mut survivors = Vec::new();
    let mut alive = 0;
    for made in run.made.iter().rev() {
        let sid = &made.user.sid;
        match undo_user(machine, acl, run, made) {
            Ok(()) => {
                if run.ledger_names(sid) {
                    survivors.push(sid.clone());
                }
            }
            Err(what) => {
                left.push(what);
                survivors.push(sid.clone());
                alive += 1;
            }
        }
    }
    survivors.reverse();
    let group_gone = run.group_made && alive == 0 && delete_group(machine, run, &mut left);
    rewrite_record(machine, req, run, &survivors, group_gone, &mut left);
    if !run.reset.is_empty()
        && let Err(error) = owned(machine, |m| m.protect_for_owner(&run.reset, &run.kept))
    {
        left.push(format!(
            "secrets.bin, which lacks the sandbox users' new passwords ({}): run set-up again",
            cause_of(&error)
        ));
    }
    left
}

/// One made user off, in the uninstall order. A `LocalLow` deny that won't come off goes with
/// the profile folder; a profile that won't go keeps the account, so a removal later finds both.
fn undo_user(
    machine: &mut impl Machine,
    acl: &mut impl AclWriter,
    run: &Run,
    made: &Made,
) -> Result<(), String> {
    let name = made.user.name.as_str();
    let mut deny_left = None;
    if let Some(profile) = &made.profile {
        let local_low = local_low(profile);
        let grants: Vec<Grant> = run
            .added
            .iter()
            .filter(|grant| grant.path() == local_low)
            .cloned()
            .collect();
        if !grants.is_empty()
            && let Err(failure) = remove_shared_write_denies(acl, &grants)
        {
            deny_left = Some(failure);
        }
    }
    if made.logged_on
        && let Err(error) = machine.delete_profile(&made.user.sid)
    {
        let deny = deny_left.map_or(String::new(), |f| format!(", its LocalLow deny ({f})"));
        return Err(format!(
            "{name}'s profile ({}){deny} and account",
            cause_of(&error)
        ));
    }
    machine
        .delete_user(name)
        .map_err(|error| format!("{name}'s account ({})", cause_of(&error)))
}

/// `<profile>\AppData\LocalLow`, as `shared_write_targets` spells it.
fn local_low(profile: &Path) -> PathBuf {
    profile.join("AppData").join("LocalLow")
}

/// Deletes the group this run made when it has no members left; whether it is gone. A group
/// that still has members (kept users this run put in it) stays and is recorded, which is not a
/// failure; a failed count or delete keeps it and is.
fn delete_group(machine: &mut impl Machine, run: &Run, left: &mut Vec<String>) -> bool {
    let Some(group) = &run.group else {
        return false;
    };
    let result = match machine.group_members(group) {
        Ok(0) => machine.delete_group(group),
        Ok(_) => return false,
        Err(error) => Err(error),
    };
    match result {
        Ok(()) => true,
        Err(error) => {
            left.push(format!(
                "the group ClauseoSandboxUsers ({})",
                cause_of(&error)
            ));
            false
        }
    }
}

/// The record written to what remains, after the users and the group: every SID the record
/// held before the run (so nothing recorded before is dropped) plus `survivors`, with this run's
/// group while it is still there, else the group before. No record at all when there was none
/// before and nothing remains. Written only when it differs from what the machine holds.
fn rewrite_record(
    machine: &mut impl Machine,
    req: &SetupRequest,
    run: &Run,
    survivors: &[Sid],
    group_gone: bool,
    left: &mut Vec<String>,
) {
    let made_group_left = run.group_made && !group_gone;
    let before = run.before.as_ref();
    let result = if before.is_none() && survivors.is_empty() && !made_group_left {
        match run.recorded {
            Some(_) => machine.delete_record().map_err(|error| cause_of(&error)),
            None => Ok(()),
        }
    } else {
        let group = if made_group_left {
            run.group.clone()
        } else {
            before
                .map(|b| b.group.clone())
                .or_else(|| run.group.clone())
        };
        let Some(group) = group else {
            // No group means nothing was made: the record is as it was.
            return;
        };
        let mut pool = before.map(|b| b.pool.clone()).unwrap_or_default();
        let new: Vec<Sid> = survivors
            .iter()
            .filter(|sid| !pool.contains(sid))
            .cloned()
            .collect();
        pool.extend(new);
        let record = MachineRecord {
            owner: req.owner.clone(),
            group,
            pool,
            data_dir: before.map_or_else(|| req.data_dir.clone(), |b| b.data_dir.clone()),
            removing: before.is_some_and(|b| b.removing),
        };
        if run.recorded() == Some(&record) {
            Ok(())
        } else {
            write_record(machine, &record)
        }
    };
    if let Err(cause) = result {
        let survivors: Vec<String> = survivors.iter().map(ToString::to_string).collect();
        left.push(format!(
            "the machine's record, which may not name what this run left ({cause}; survivors: \
             [{}])",
            survivors.join(", ")
        ));
    }
}
