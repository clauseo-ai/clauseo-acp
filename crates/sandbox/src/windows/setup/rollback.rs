//! A failed set-up's rollback (D-B7). Users this run made come off in the uninstall order: their
//! `LocalLow` deny, their profile, their account. The record is rewritten to the survivors, the
//! group goes only if this run made it and it is empty, and every other deny stays (D12).
//! Nothing that existed before the run is removed.
//!
//! Two things a rollback can't undo: a re-run's new passwords for the recorded users (they stay
//! set, and `secrets.bin` holds them only if step 10 ran), and the denies, which stay on
//! purpose: a deny naming a group or user that is gone does no harm. A `secrets.bin` step 10
//! wrote stays too, with blobs for users now deleted, which nothing can use.

use std::path::{Path, PathBuf};

use super::SetupRequest;
use super::create::write_record;
use super::machine::Machine;
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

/// What a run has changed so far: what its rollback reads.
#[derive(Debug)]
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
    let group_goes = run.group_made && alive == 0 && group_empty(machine, run, &mut left);
    if run.recorded.is_some() {
        rewrite_record(machine, req, run, &survivors, group_goes, &mut left);
    }
    if let (true, Some(group)) = (group_goes, &run.group)
        && let Err(error) = machine.delete_group(group)
    {
        left.push(format!(
            "the group ClauseoSandboxUsers: {}",
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

/// Whether the group this run made has no members left; a failed count keeps it.
fn group_empty(machine: &mut impl Machine, run: &Run, left: &mut Vec<String>) -> bool {
    let Some(group) = &run.group else {
        return false;
    };
    match machine.group_members(group) {
        Ok(0) => true,
        Ok(_) => false,
        Err(error) => {
            left.push(format!(
                "the group ClauseoSandboxUsers ({})",
                cause_of(&error)
            ));
            false
        }
    }
}

/// The record back to the survivors: the one before the run (every SID it listed, so nothing
/// recorded before is dropped) plus `survivors`; or no record at all when there was none before
/// and nothing survives. The group recorded is the one before when this run's group goes.
fn rewrite_record(
    machine: &mut impl Machine,
    req: &SetupRequest,
    run: &Run,
    survivors: &[Sid],
    group_goes: bool,
    left: &mut Vec<String>,
) {
    let result = match (&run.before, &run.group) {
        (None, _) if survivors.is_empty() => {
            machine.delete_record().map_err(|error| cause_of(&error))
        }
        (_, None) => Ok(()),
        (before, Some(group)) => {
            let mut pool = before.as_ref().map(|b| b.pool.clone()).unwrap_or_default();
            let new: Vec<Sid> = survivors
                .iter()
                .filter(|sid| !pool.contains(sid))
                .cloned()
                .collect();
            pool.extend(new);
            let group = match before {
                Some(before) if group_goes => before.group.clone(),
                _ => group.clone(),
            };
            let record = MachineRecord {
                owner: req.owner.clone(),
                group,
                pool,
                data_dir: before
                    .as_ref()
                    .map_or_else(|| req.data_dir.clone(), |b| b.data_dir.clone()),
                removing: before.as_ref().is_some_and(|b| b.removing),
            };
            write_record(machine, &record)
        }
    };
    if let Err(cause) = result {
        let survivors: Vec<String> = survivors.iter().map(ToString::to_string).collect();
        left.push(format!(
            "the machine's record, which may still list this run's users ({cause}; survivors: \
             [{}])",
            survivors.join(", ")
        ));
    }
}
