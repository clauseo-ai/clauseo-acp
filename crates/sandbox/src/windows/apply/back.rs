//! The way back (S15 PR3): a launch's rollback, `forget_matter`, and the start's `reconcile`.
//!
//! Each takes entries back through [`Ledger::take_back`], which under one hold of the ledger's
//! lock sets them `pending` on disk, decides which are shared from the file as read then, and
//! removes the rest's permission entries (the review's M3 and M4). The trees are then walked
//! again, outside the lock, so the inherited copies go too, and only then are the entries whose
//! permission entry is gone dropped ([`Ledger::drop_gone`]). A crash anywhere leaves `pending`
//! entries, which [`reconcile`] walks and settles.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::PathBuf;

use clauseo_protocol::MatterId;

use super::{Run, reach_of, refused};
use crate::Error;
use crate::windows::acl_plan::{AdmittedPlan, Inherit, Sid, in_folder};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{Entry, EntryId, Forgotten, Ledger, Reconciled, Take, TakenBack};
use crate::windows::writer::{AclWriter, ApplyFailure, Reach};

/// Removes `entry`'s permission entry, and puts back the explicit label its path had before
/// ours (L4).
pub(super) fn remove_entry(acl: &mut dyn AclWriter, entry: &Entry) -> io::Result<()> {
    let grant = entry.grant();
    acl.remove(&grant)?;
    if let Some(label) = entry.prior_label {
        acl.restore_label(&grant, label)?;
    }
    Ok(())
}

/// Takes back the entries this run added: the ones it wrote are removed (never one another
/// entry names), the others dropped. A removal or walk that fails leaves its entry `pending`.
pub(super) fn roll_back(
    ledger: &mut Ledger,
    plan: &AdmittedPlan,
    acl: &mut dyn AclWriter,
    run: &Run,
) {
    let created: BTreeSet<EntryId> = plan.created().iter().copied().collect();
    let written: BTreeSet<EntryId> = run
        .written
        .iter()
        .filter_map(|&i| plan.entries().get(i))
        .copied()
        .collect();
    let pick = |entry: &Entry| {
        created.contains(&entry.id).then(|| {
            if written.contains(&entry.id) {
                Take::Remove
            } else {
                Take::Drop
            }
        })
    };
    let taken = ledger.take_back(pick, |entry| remove_entry(acl, entry));
    match taken {
        Ok(taken) => {
            if let Err(failure) = finish(ledger, acl, &taken, &[]) {
                tracing::warn!(%failure, "a rollback's walks or drops failed; entries stay pending");
            }
        }
        Err(error) => {
            let error = error.kind();
            tracing::warn!(%error, "a rollback couldn't take its entries back");
        }
    }
}

/// Walks the trees of `taken`'s removed entries again (`reclaim`'s with the matter's pool user
/// handing back what it made, the rest plainly), then drops each removed entry whose tree was
/// walked and whose permission entry is gone. Returns how many were dropped.
pub(super) fn finish(
    ledger: &mut Ledger,
    acl: &mut dyn AclWriter,
    taken: &TakenBack,
    reclaim: &[(PathBuf, Vec<Sid>)],
) -> Result<usize, ApplyFailure> {
    let mut trees: BTreeMap<(usize, PathBuf), Reach> = BTreeMap::new();
    for entry in &taken.removed {
        if entry.inherit == Inherit::ThisOnly {
            continue;
        }
        let key = (entry.path.components().count(), entry.path.clone());
        let slot = trees.entry(key).or_insert(reach_of(entry.inherit));
        if reach_of(entry.inherit) == Reach::Tree {
            *slot = Reach::Tree;
        }
    }
    for (root, _) in reclaim {
        trees.insert((root.components().count(), root.clone()), Reach::Tree);
    }
    let mut walked: Vec<PathBuf> = Vec::new();
    let mut failed: Option<ApplyFailure> = None;
    for ((_, root), reach) in &trees {
        let pool = reclaim.iter().find(|(r, _)| r == root).map(|(_, p)| p);
        let inside = walked.iter().any(|w| w != root && in_folder(root, w));
        if inside && pool.is_none() {
            walked.push(root.clone());
            continue;
        }
        let done = match pool {
            Some(pool) => acl.reclaim(root, pool),
            None => acl.propagate(root, *reach, None),
        };
        match done {
            Ok(_) => walked.push(root.clone()),
            Err(failure) => {
                tracing::warn!(%failure, "a walk after a take-back failed; its entries stay pending");
                failed.get_or_insert(failure);
            }
        }
    }
    let ids: Vec<EntryId> = taken
        .removed
        .iter()
        .filter(|e| e.inherit == Inherit::ThisOnly || walked.contains(&e.path))
        .map(|e| e.id)
        .collect();
    let dropped = ledger
        .drop_gone(&ids, |entry| acl.present(&entry.grant()))
        .map_err(|e| ApplyFailure::Ledger(e.kind()))?;
    match failed {
        Some(failure) => Err(failure),
        None => Ok(dropped),
    }
}

/// Forgets `matter` without admin: takes back each of its entries (labels included, the
/// lawyer's own label put back) unless another matter's entry names it, hands what its pool
/// user made in its matter, state and notes folders back to the desktop user
/// ([`AclWriter::reclaim`], the review's M5), walks each tree once, and drops the entries whose
/// permission entry is gone. An entry whose removal or walk failed stays `pending` for the next
/// try.
pub fn forget_matter(
    ledger: &mut Ledger,
    matter: MatterId,
    acl: &mut dyn AclWriter,
) -> Result<Forgotten, Error> {
    let entries = ledger.matter_entries(matter);
    let pool: Vec<Sid> = entries
        .iter()
        .filter(|e| e.rights.is_allow())
        .map(|e| e.sid.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let own = [PathKind::Matter, PathKind::State, PathKind::Notes];
    let reclaim: Vec<(PathBuf, Vec<Sid>)> = entries
        .iter()
        .filter(|e| own.contains(&e.kind) && e.rights.is_allow() && e.inherit == Inherit::Tree)
        .map(|e| e.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|path| (path, pool.clone()))
        .collect();
    let failed = |e: io::Error| refused(PathKind::Matter, ApplyFailure::Ledger(e.kind()));
    let taken = ledger
        .take_back(
            |entry| (entry.matter == matter).then_some(Take::Remove),
            |entry| remove_entry(acl, entry),
        )
        .map_err(failed)?;
    finish(ledger, acl, &taken, &reclaim).map_err(|f| refused(PathKind::Matter, f))?;
    Ok(Forgotten {
        removed: taken.removed.len(),
        kept: taken.kept,
        shared: taken.shared,
    })
}

/// At start: walks each `pending` entry's tree again (an entry is `pending` when a launch, a
/// rollback or a forget stopped partway, the review's M2), then marks the ones whose permission
/// entry is there `applied` and drops the rest. An entry whose check or walk fails stays
/// `pending` for the next start.
pub fn reconcile(ledger: &mut Ledger, acl: &mut dyn AclWriter) -> Result<Reconciled, Error> {
    let failed = |e: io::Error| refused(PathKind::Matter, ApplyFailure::Ledger(e.kind()));
    let pending = ledger.pending().map_err(failed)?;
    let mut done = Reconciled::default();
    let (mut there, mut gone) = (Vec::new(), Vec::new());
    for entry in pending {
        match acl.present(&entry.grant()) {
            Ok(true) => there.push(entry),
            Ok(false) => gone.push(entry),
            Err(error) => {
                let (kind, error) = (entry.kind, error.kind());
                tracing::warn!(%kind, %error, "a pending ACL entry wasn't checked");
                done.unchecked += 1;
            }
        }
    }
    let mut walked: BTreeMap<PathBuf, bool> = BTreeMap::new();
    for entry in there.iter().chain(&gone) {
        if entry.inherit == Inherit::ThisOnly || walked.contains_key(&entry.path) {
            continue;
        }
        let ok = acl
            .propagate(&entry.path, reach_of(entry.inherit), None)
            .inspect_err(|failure| tracing::warn!(%failure, "a reconcile walk failed"))
            .is_ok();
        walked.insert(entry.path.clone(), ok);
    }
    let settled = |entry: &&Entry| {
        entry.inherit == Inherit::ThisOnly || walked.get(&entry.path).copied().unwrap_or(false)
    };
    let applied: Vec<EntryId> = there.iter().filter(settled).map(|e| e.id).collect();
    let dropped: Vec<EntryId> = gone.iter().filter(settled).map(|e| e.id).collect();
    done.unchecked += there.len() + gone.len() - applied.len() - dropped.len();
    ledger.mark_walked(&applied).map_err(failed)?;
    done.applied = applied.len();
    done.dropped = ledger
        .drop_gone(&dropped, |entry| acl.present(&entry.grant()))
        .map_err(failed)?;
    Ok(done)
}
