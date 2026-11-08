//! A session granted a read of another matter's whole folder lists it (R18): the fence's deny
//! of the session's pool user on that folder alone is lifted for the session, and put back when
//! the grant goes. The fence's deny for what is below the folder stays, so a child the grant
//! doesn't allow (a refused name) stays closed even while the folder's deny is off.
//!
//! The deny is the fenced matter's ledger entry. The lift is marked on it first
//! ([`set_lifted`], under the lock), then the deny removed; the deny is put back first, then the
//! mark cleared. So a crash leaves the mark, and the restore, run again, finds the deny there or
//! writes it. Nothing reads the mark as a missing fence: [`Ledger::fence_applied`] reads the
//! entry, still `applied`. A launch that writes the fence again (the fenced matter's own, or one
//! that plans its fence) puts the deny back while the session runs: the session can't list the
//! folder until its next launch lifts it again, and the restore finds it there.

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
        let marked = set_lifted(ledger, deny.id, Some(matter));
        if let Err(error) = marked {
            let error = error.kind();
            tracing::warn!(%error, "a fence's deny wasn't marked lifted; it stays");
            continue;
        }
        if let Err(failure) = acl.remove(&deny.grant()) {
            tracing::warn!(%failure, "a fence's deny wasn't lifted; the folder stays unlisted");
        }
    }
}

/// Puts back each lifted fence deny `pick` takes, then clears its mark. A deny there already
/// (a launch wrote the fence again) is only unmarked; one whose entry is being removed (its
/// matter is being forgotten) is only unmarked. Returns the folders whose deny is back. Each
/// one whose write fails keeps its mark, so the next call tries again, and refuses.
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
        set_lifted(ledger, deny.id, None).map_err(failed)?;
        back.push(deny.path);
    }
    match refusal {
        Some(error) => Err(error),
        None => Ok(back),
    }
}

/// [`restore`] of every deny `matter`'s sessions lifted.
pub(super) fn restore_matter(
    ledger: &mut Ledger,
    acl: &mut dyn AclWriter,
    matter: MatterId,
) -> Result<Vec<std::path::PathBuf>, Error> {
    restore(ledger, acl, |e| e.lifted == Some(matter))
}
