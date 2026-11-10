//! Step 6 of `--remove`: the ledger's entries. Exactly what is touched depends on who acts.
//!
//! **As the owner** (the parent is the record's owner; per-user, or per-machine run by the
//! owner): everything runs on the thread acting as the owner (`Removal::as_owner`) with a
//! standard user's token: the parent's own, or, when the parent is elevated (a per-machine
//! uninstaller is), a copy with Administrators deny-only, Medium integrity and no privilege but
//! `SeChangeNotifyPrivilege` (`remove::acting_token`). So it can do nothing the owner's standard
//! token couldn't, even with a ledger malware running as the owner wrote. A parent whose token
//! can't be lowered gets the administrator's path below instead in a per-machine run; a
//! per-user run stops before this. The record's `DataDir` is checked as `--data-dir` is
//! (`Removal::owners_ledger`); then every matter with a trusted entry or a hand-back owed is
//! forgotten (`apply::forget_matter`: its entries taken back, the lawyer's own labels put
//! back, what its pool users made handed back to the owner, its trees walked), then, once nothing
//! of it is left in the ledger, its set-aside bookkeeping deleted (`Removal::forget_set_aside`:
//! when the matter's set-aside folder holds only `record.json` with no open session,
//! `set-aside.lock` and a `manifest.json` with nothing outstanding, those and the folder go;
//! otherwise nothing; a failure is logged, never fatal). Anything left
//! (an entry whose removal failed, a hand-back still owed, a kept entry naming a pool SID) stops
//! the run, so the record stays and the ledger keeps trusting its entries.
//!
//! **As an administrator** (per-machine, the owner didn't ask; LOCAL SYSTEM under Intune): no
//! impersonation is possible, and the ledger and `DataDir` are files the owner can write, so a
//! crafted ledger may steer an administrator onto nothing but taking our own entries off
//! (K1). The folders it may name include ones the owner doesn't own (Administrators' or
//! SYSTEM's, `remove::owners`), where it can remove only the pool's entries, which name our
//! SIDs alone, and a Low label only for an owner who is an administrator. The ledger is read
//! from a `DataDir` checked as `Removal::admins_ledger` says (opened relative to the checked
//! `sandbox` folder's handle) and is never written, and the only change ever made is
//! [`AclWriter::remove`] of one explicit access entry, on an entry that:
//!
//! - names one of the record's pool SIDs or its group (from `HKLM`, which only administrators
//!   write), never a matter SID, the Low label, `OWNER RIGHTS` or anyone else;
//! - is an allow or a deny, never a label (a recorded earlier label is never put back);
//! - records no file by ID (a credential link's entry), which would be opened by its ID with no
//!   path to check;
//! - lies in a matter folder the ledger records ([`PathKind::Matter`], an allow to the whole tree
//!   for a pool SID), not a drive's root;
//! - and whose path, checked right before, is a folder reached with no junction or link, owned by
//!   the record's owner, Administrators or SYSTEM (`owners::matter_folder`: an elevated process
//!   makes Administrators the owner of the folders it creates), on a local volume that keeps
//!   permissions (`Removal::admin_target`).
//!
//! Besides, each recorded matter folder that passes that check has the Low label the ledger
//! records on it taken off, through the same [`AclWriter::remove`] (the label alone, the folder
//! re-opened without following a link, its final path checked, no walk): removing a label only
//! tightens, and no label is ever written or put back. The inherited copies below stay. On a
//! folder the owner doesn't own, the label comes off only when the owner is in Administrators
//! (`owners::label_off`, `Removal::owner_in_administrators`): otherwise a crafted ledger could
//! strip another program's Low label (say, a `ProgramData` folder its Low processes write to).
//! It stays, with a log line, and counts as left.
//!
//! No walk, no label written, no hand-back, no ownership change, no add, no delete. Whatever
//! else the ledger lists stays, counted in [`LedgerWork::AsAdmin`]: an entry naming a dead SID
//! holds nothing (K10), and the record goes anyway (D-B9 as ruled).

use std::path::Path;

use clauseo_protocol::MatterId;

use super::owners::label_off;
use super::{Acting, Failed, FolderOwner, Removal, RemoveStep};
use crate::windows::acl_plan::{Inherit, in_folder};
use crate::windows::admit::PathKind;
use crate::windows::apply::forget_matter;
use crate::windows::ledger::{Entry, Ledger};
use crate::windows::pool::MachineRecord;
use crate::windows::setup::outcome::cause_of;
use crate::windows::sids::Sid;
use crate::windows::writer::AclWriter;

/// What step 6 did. Counts only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LedgerWork {
    /// Nothing to do: no data folder, no ledger, or nothing in it.
    #[default]
    Nothing,
    /// As the owner: the matters forgotten and the entries whose permission entry was removed.
    AsOwner {
        /// Matters forgotten.
        matters: usize,
        /// Entries removed.
        removed: usize,
    },
    /// As an administrator for an absent owner: entries whose permission entry was removed,
    /// and entries left as they are.
    AsAdmin {
        /// Entries removed.
        removed: usize,
        /// Entries left: outside the limits, or refused by the path check or the removal.
        left: usize,
    },
    /// As an administrator, the ledger wasn't read; why. The record goes anyway.
    Skipped(String),
}

/// Step 6, as the module says.
pub(super) fn run(
    machine: &mut impl Removal,
    acl: &mut impl AclWriter,
    record: &MachineRecord,
    acting: Acting,
) -> Result<LedgerWork, Failed> {
    let trusted: Vec<Sid> = record
        .pool
        .iter()
        .chain(std::iter::once(&record.group))
        .cloned()
        .collect();
    match acting {
        Acting::Owner => as_owner(machine, acl, record, &trusted),
        Acting::Admin => Ok(as_admin(machine, acl, record, &trusted)),
    }
}

/// As the owner: every matter forgotten, nothing left.
fn as_owner(
    machine: &mut impl Removal,
    acl: &mut impl AclWriter,
    record: &MachineRecord,
    trusted: &[Sid],
) -> Result<LedgerWork, Failed> {
    let worked = machine.as_owner(|m| {
        let Some(mut ledger) = m
            .owners_ledger(&record.data_dir, trusted)
            .map_err(|e| cause_of(&e))?
        else {
            return Ok(LedgerWork::Nothing);
        };
        let matters = matters_of(&ledger);
        if matters.is_empty() && left(&ledger, trusted).is_none() {
            return Ok(LedgerWork::Nothing);
        }
        let mut removed = 0;
        for matter in &matters {
            let forgotten = forget_matter(&mut ledger, *matter, &mut *acl, m.surroundings())
                .map_err(|e| e.to_string())?;
            removed += forgotten.removed;
            // Only a matter fully forgotten (nothing of it left in the ledger) loses its
            // set-aside bookkeeping, so the uninstaller keeps the set-aside only for the lawyer's
            // files. Left, it only means the box says Kept.
            if forgotten_whole(&ledger, *matter)
                && let Err(error) = m.forget_set_aside(matter)
            {
                let error = cause_of(&error);
                tracing::warn!(%error, "a forgotten matter's set-aside bookkeeping stays");
            }
        }
        match left(&ledger, trusted) {
            Some(why) => Err(why),
            None => Ok(LedgerWork::AsOwner {
                matters: matters.len(),
                removed,
            }),
        }
    });
    match worked {
        Ok(Ok(work)) => Ok(work),
        Ok(Err(cause)) => Err((RemoveStep::Ledger, cause)),
        Err(error) => Err((RemoveStep::Ledger, cause_of(&error))),
    }
}

/// Whether the ledger holds nothing more of `matter`: no entry, no hand-back owed. An entry
/// whose removal failed stays (`forget_matter`), and so does the matter's set-aside record.
fn forgotten_whole(ledger: &Ledger, matter: MatterId) -> bool {
    !ledger.entries().any(|entry| entry.matter == matter)
        && !ledger
            .handbacks_owed()
            .iter()
            .any(|owed| owed.matter == matter)
}

/// Every matter the ledger names in a trusted entry or a hand-back owed, each once.
fn matters_of(ledger: &Ledger) -> Vec<MatterId> {
    let owed = ledger.handbacks_owed().into_iter().map(|owed| owed.matter);
    let mut matters: Vec<MatterId> = Vec::new();
    for matter in ledger.entries().map(|entry| entry.matter).chain(owed) {
        if !matters.contains(&matter) {
            matters.push(matter);
        }
    }
    matters
}

/// What the ledger still holds that the record must outlive, in words; `None` when nothing:
/// a trusted entry, a hand-back owed, or a kept entry naming one of `trusted`.
fn left(ledger: &Ledger, trusted: &[Sid]) -> Option<String> {
    let entries = ledger.entries().count();
    let owed = ledger.handbacks_owed().len();
    let named = trusted.iter().any(|sid| ledger.names_sid(sid));
    (entries > 0 || owed > 0 || named).then(|| {
        format!(
            "the ledger still lists {entries} entries and {owed} hand-backs owed{}",
            if named {
                ", and names a sandbox user"
            } else {
                ""
            }
        )
    })
}

/// As an administrator for an absent owner: the limited removal the module lists. Never fails
/// the run.
fn as_admin(
    machine: &mut impl Removal,
    acl: &mut impl AclWriter,
    record: &MachineRecord,
    trusted: &[Sid],
) -> LedgerWork {
    let ledger = match machine.admins_ledger(&record.data_dir, &record.owner, trusted) {
        Ok(Some(ledger)) => ledger,
        Ok(None) => return LedgerWork::Nothing,
        Err(error) => return LedgerWork::Skipped(cause_of(&error)),
    };
    let all: Vec<&Entry> = ledger.entries().collect();
    if all.is_empty() {
        return LedgerWork::Nothing;
    }
    // Each path checked once, right before its first removal.
    let mut checked: Vec<(&Path, Option<FolderOwner>)> = Vec::new();
    let owner = &record.owner;
    let mut removed = 0;
    for entry in admin_targets(&all, trusted) {
        let path = entry.path.as_path();
        if passes(machine, &mut checked, path, owner).is_some()
            && acl.remove(&entry.grant()).is_ok()
        {
            removed += 1;
        }
    }
    // Asked once, and only for a root that isn't the owner's.
    let mut in_admins = None;
    for root in matter_roots(&all, trusted) {
        let labels: Vec<&&Entry> = all
            .iter()
            .filter(|e| e.rights.is_label() && e.file.is_none() && same_path(&e.path, root))
            .collect();
        if labels.is_empty() {
            continue;
        }
        let Some(folder) = passes(machine, &mut checked, root, owner) else {
            continue;
        };
        let owner_in_admins =
            || *in_admins.get_or_insert_with(|| machine.owner_in_administrators(owner));
        if !label_off(folder, owner_in_admins) {
            tracing::warn!(
                "a matter folder's Low label stays: Administrators or SYSTEM owns the folder, \
                 and the record's owner isn't an administrator"
            );
            continue;
        }
        for label in labels {
            if acl.remove(&label.grant()).is_ok() {
                removed += 1;
            }
        }
    }
    LedgerWork::AsAdmin {
        removed,
        left: all.len() - removed,
    }
}

/// Who owns `path` when it passes `Removal::admin_target` against `owner`; `None` when it
/// doesn't. Asked once per path.
fn passes<'p>(
    machine: &mut impl Removal,
    checked: &mut Vec<(&'p Path, Option<FolderOwner>)>,
    path: &'p Path,
    owner: &Sid,
) -> Option<FolderOwner> {
    if let Some((_, found)) = checked.iter().find(|(seen, _)| *seen == path) {
        return *found;
    }
    let found = machine.admin_target(path, owner).ok();
    checked.push((path, found));
    found
}

/// The matter folders `entries` record for one of `trusted`: each path with a trusted
/// [`PathKind::Matter`] allow to the whole tree, not a drive's root, each once.
fn matter_roots<'l>(entries: &[&'l Entry], trusted: &[Sid]) -> Vec<&'l Path> {
    let mut roots: Vec<&Path> = Vec::new();
    for e in entries {
        let root = e.kind == PathKind::Matter
            && e.rights.is_allow()
            && e.inherit == Inherit::Tree
            && trusted.contains(&e.sid)
            && e.path.parent().is_some();
        if root && !roots.iter().any(|seen| same_path(seen, &e.path)) {
            roots.push(e.path.as_path());
        }
    }
    roots
}

/// Whether `a` and `b` name one path, as NTFS compares names.
fn same_path(a: &Path, b: &Path) -> bool {
    in_folder(a, b) && in_folder(b, a)
}

/// The entries of `entries` an administrator may remove for an absent owner, before the path
/// check: each names one of `trusted` (or is `OWNER RIGHTS`' entry), is an allow or a deny,
/// records no file, isn't IT's own (`Entry::found`), and lies in (or is) one of the matter
/// folders [`matter_roots`] finds.
pub(super) fn admin_targets<'l>(entries: &[&'l Entry], trusted: &[Sid]) -> Vec<&'l Entry> {
    let roots = matter_roots(entries, trusted);
    entries
        .iter()
        .filter(|e| {
            (trusted.contains(&e.sid) || e.rights.fixed_subject() == Some(e.sid.as_str()))
                && !e.rights.is_label()
                && !e.found
                && e.file.is_none()
                && roots.iter().any(|root| in_folder(&e.path, root))
        })
        .copied()
        .collect()
}
