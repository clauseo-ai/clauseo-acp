//! A session granted a read of another matter's whole folder lists it (R18): the fence's deny
//! of the session's pool user on that folder alone is lifted for the session, and put back when
//! the grant goes. The fence's deny for what is below the folder stays, so a child the grant
//! doesn't allow (a refused name) stays closed even while the folder's deny is off.
//!
//! Lifted, the deny of reads and writes is swapped for a deny of writes alone, on the folder
//! alone, for the same pool user ([`stand_in`]): the folder inherits `Users`' right to make
//! files and folders from `C:\`, which the Low label doesn't stop for a Low token, so with no
//! deny there an agent could plant a `CLAUDE.md` or `.claude\` for that matter's agent to load,
//! and own it. The swap writes the new deny before it removes the old one, and the restore puts
//! the old one back before it removes the new one: at every step one of them is there, so the
//! folder is never writable, and the most a crash leaves is its names listable.
//!
//! The deny is the fenced matter's ledger entry. The lift is marked on it first
//! ([`set_lifted`], under the lock), and the mark stands for the stand-in too; then the swap.
//! No deny is touched unless the file as saved carries its mark: an entry another process
//! removed or began removing in between takes none, and its deny stays.
//! The restore swaps back, then clears the mark. So a crash leaves the mark, and the restore,
//! run again, finds the deny there or writes it, and removes the stand-in if it is there.
//! Nothing reads the mark as a missing fence: [`Ledger::fence_applied`] reads the entry, still
//! `applied`. A launch that writes the fence again (the fenced matter's own, or one that plans
//! its fence) puts the deny back while the session runs: the session can't list the folder until
//! its next launch lifts it again, and the restore finds it there.

use std::path::Path;

use clauseo_protocol::MatterId;

use super::refused;
use crate::Error;
use crate::windows::acl_plan::{AdmittedPlan, Grant, Inherit, Rights};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{Entry, EntryState, Ledger, liftable, set_lifted};
use crate::windows::writer::{AclWriter, ApplyFailure};

/// After `plan` applied: lifts each fence deny on another matter's folder alone, naming the
/// launch's own subjects, where `plan` allows that folder itself as a read of another matter
/// (a read of the whole folder; a deeper folder carries no fence entry). A failure is logged:
/// the deny stays, and the session can't list the folder.
pub(super) fn lift(ledger: &mut Ledger, plan: &AdmittedPlan, acl: &mut dyn AclWriter) {
    let matter = plan.matter();
    let sids = plan.assigned_to(matter);
    let folders: Vec<&Path> = plan
        .grants()
        .iter()
        .filter(|g| {
            g.kind() == PathKind::OtherMatterRead
                && g.rights() == Rights::ReadAndExecute
                && g.inherit() == Inherit::ThisOnly
        })
        .map(Grant::path)
        .collect();
    if folders.is_empty() {
        return;
    }
    let denies: Vec<Entry> = ledger
        .entries()
        .filter(|e| {
            liftable(e)
                && e.state == EntryState::Applied
                && e.matter != matter
                && sids.contains(&e.sid)
                && folders.contains(&e.path.as_path())
        })
        .cloned()
        .collect();
    for deny in denies {
        if let Err(error) = set_lifted(ledger, deny.id, Some(matter)) {
            let error = error.kind();
            tracing::warn!(%error, "a fence's deny wasn't marked lifted; it stays");
            continue;
        }
        // The file as saved must carry the mark: the entry may be gone, being removed, or no
        // longer liftable since it was read. Without the mark nothing would put the deny back.
        let ours = |e: &Entry| e.id == deny.id && e.lifted == Some(matter);
        if !ledger.entries().any(ours) {
            tracing::warn!("a fence's deny wasn't marked lifted; it stays");
            continue;
        }
        // The deny of writes first: the folder is never left open to them (see the module).
        if let Err(failure) = acl.add(&stand_in(&deny)) {
            tracing::warn!(%failure, "a fence's stand-in wasn't written; the deny stays");
            continue;
        }
        if let Err(failure) = acl.remove(&deny.grant()) {
            tracing::warn!(%failure, "a fence's deny wasn't lifted; the folder stays unlisted");
        }
    }
}

/// What stands in for a lifted `deny` on its folder: a deny of every write (`DenyWrite`: write
/// data or add a file, append or add a folder, extended attributes, attributes, delete a child,
/// `DELETE`, `WRITE_DAC`, `WRITE_OWNER`) for the same pool user, on the folder alone.
pub(crate) fn stand_in(deny: &Entry) -> Grant {
    Grant {
        rights: Rights::DenyWrite,
        ..deny.grant()
    }
}

/// Puts back each lifted fence deny `pick` takes, removes its [`stand_in`], then clears its
/// mark. A deny there already (a launch wrote the fence again) isn't written twice; one whose
/// entry is being removed (its matter is being forgotten) isn't written. Returns the folders
/// whose deny is back. Each one whose write fails keeps its mark (and the stand-in), so the next
/// call tries again, and refuses.
pub(super) fn restore(
    ledger: &mut Ledger,
    acl: &mut dyn AclWriter,
    pick: impl Fn(&Entry) -> bool,
) -> Result<Vec<std::path::PathBuf>, Error> {
    let kind = PathKind::OtherMatterRead;
    let failed = |e: std::io::Error| refused(kind, ApplyFailure::Ledger(e.kind()));
    // The file as it is now: another process may have lifted or restored since.
    ledger.pending().map_err(failed)?;
    let lifted: Vec<Entry> = ledger
        .entries()
        .filter(|e| e.lifted.is_some() && pick(e))
        .cloned()
        .collect();
    let mut back = Vec::new();
    let mut refusal = None;
    for deny in lifted {
        if deny.state != EntryState::Removing
            && let Err(failure) = acl.add(&deny.grant())
        {
            tracing::warn!(%failure, "a lifted fence deny wasn't put back");
            refusal.get_or_insert(refused(kind, failure));
            continue;
        }
        // Only once the deny is back: the folder is never left open to writes.
        if let Err(failure) = acl.remove(&stand_in(&deny)) {
            tracing::warn!(%failure, "a lifted fence deny's stand-in wasn't removed");
            refusal.get_or_insert(refused(kind, failure));
            continue;
        }
        set_lifted(ledger, deny.id, None).map_err(failed)?;
        back.push(deny.path);
    }
    match refusal {
        Some(error) => Err(error),
        None => Ok(back),
    }
}

/// [`restore`] of every deny `matter`'s sessions lifted, and of `matter`'s own denies another
/// matter's session lifted: its forget removes them, and their stand-ins with them.
pub(super) fn restore_matter(
    ledger: &mut Ledger,
    acl: &mut dyn AclWriter,
    matter: MatterId,
) -> Result<Vec<std::path::PathBuf>, Error> {
    restore(ledger, acl, |e| {
        e.lifted == Some(matter) || e.matter == matter
    })
}
