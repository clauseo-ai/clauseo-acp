//! The way back (S15 PR3): a launch's rollback, `forget_matter`, and the start's `reconcile`.
//!
//! Each takes entries back through [`Ledger::take_back`], which under one hold of the ledger's
//! lock sets them `pending` on disk, decides which are shared from the file as read then, and
//! removes the rest's permission entries (the review's M3 and M4). The trees are then walked
//! again, outside the lock, so the inherited copies go too, and only then are the entries whose
//! permission entry is gone dropped ([`Ledger::drop_gone`]). A crash anywhere leaves `pending`
//! entries, which [`reconcile`] walks and settles.
//!
//! A Low label on another name for the same file counts as the same permission entry
//! ([`AclWriter::same_file`]): the label belongs to the file, so forgetting one matter whose
//! credential link is the same file as another's leaves it (S15 `PR3b` round 2, N7).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::walk::{OWN_KINDS, Refused, clearing, reach_of};
use super::{Run, Surroundings, refused};
use crate::Error;
use crate::windows::acl_plan::{AdmittedPlan, Grant, Inherit, Sid, in_folder};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{
    Entry, EntryId, Forgotten, Ledger, Reconciled, Shares, Take, TakenBack,
};
use crate::windows::writer::{AclWriter, ApplyFailure, Reach};

/// Removes `entry`'s permission entry, and puts back the explicit label its path had before
/// ours (L4). A credential link's entry is removed from the credentials file it recorded
/// ([`on_file`]).
pub(super) fn remove_entry(acl: &mut dyn AclWriter, entry: &Entry) -> io::Result<()> {
    let Some(grant) = on_file(acl, entry)? else {
        return Ok(());
    };
    acl.remove(&grant)?;
    if let Some(label) = entry.prior_label {
        acl.restore_label(&grant, label)?;
    }
    Ok(())
}

/// The grant through which `entry`'s permission entry is taken back or checked: its own, or,
/// for an entry that recorded the file it was written on (a credential link's, round 4, L5),
/// the same grant on that file's recorded path, once the file there, opened as itself, has the
/// recorded file ID. Never through the link, which may name another file by now. `None` when
/// nothing is at that path: a file that is gone took its entries with it, as a missing path
/// does for [`AclWriter::remove`]. Another file there is [`ApplyFailure::Changed`]: the entry
/// stays for the lawyer to see. The recorded path is outside the state folder (N8), out of the
/// agent's reach, so it can't swap the file between the check and the write.
fn on_file(acl: &mut dyn AclWriter, entry: &Entry) -> io::Result<Option<Grant>> {
    let grant = entry.grant();
    let Some(file) = &entry.file else {
        return Ok(Some(grant));
    };
    match acl.file_id(&file.path)? {
        None => Ok(None),
        Some(id) if id == file.id => Ok(Some(Grant {
            path: file.path.clone(),
            ..grant
        })),
        Some(_) => Err(ApplyFailure::Changed.into()),
    }
}

/// Whether `entry`'s permission entry is there, on the file [`on_file`] finds.
pub(super) fn present_entry(acl: &mut dyn AclWriter, entry: &Entry) -> io::Result<bool> {
    match on_file(acl, entry)? {
        Some(grant) => acl.present(&grant),
        None => Ok(false),
    }
}

/// Which names are one file, through the writer that also removes the entries.
struct Files<'a, 'b>(&'a RefCell<&'b mut dyn AclWriter>);

impl Shares for Files<'_, '_> {
    fn same_file(&mut self, a: &Path, b: &Path) -> bool {
        self.0.borrow_mut().same_file(a, b).unwrap_or(false)
    }
}

/// [`Ledger::take_back_with`] through `acl`: the entries `pick` takes, a Low label on another
/// name for the same file counted as shared, each removal through [`remove_entry`]; with
/// `owe`, the hand-backs that matter's forget owes recorded first.
pub(super) fn take_back(
    ledger: &mut Ledger,
    owe: Option<MatterId>,
    acl: &mut dyn AclWriter,
    pick: impl FnMut(&Entry) -> Option<Take>,
) -> io::Result<TakenBack> {
    let cell = RefCell::new(acl);
    let mut files = Files(&cell);
    ledger.take_back_with(owe, pick, &mut files, |entry| {
        remove_entry(&mut **cell.borrow_mut(), entry)
    })
}

/// Takes back the entries this run added: the ones it wrote are removed (never one another
/// entry names), the others dropped. A removal or walk that fails leaves its entry `pending`.
pub(super) fn roll_back(
    ledger: &mut Ledger,
    plan: &AdmittedPlan,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
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
    match take_back(ledger, None, acl, pick) {
        Ok(taken) => {
            if let Err(refused) = finish(ledger, acl, around, &taken, &[]) {
                let failure = refused.failure;
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
/// walked and whose permission entry is gone. Returns how many were dropped. A child the lawyer
/// can't open inside one of `reclaim`'s trees is set aside and the walk run again (`walk.rs`);
/// anywhere else it fails the walk.
pub(super) fn finish(
    ledger: &mut Ledger,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
    taken: &TakenBack,
    reclaim: &[(PathBuf, Vec<Sid>)],
) -> Result<usize, Refused> {
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
    let own: Vec<PathBuf> = reclaim.iter().map(|(root, _)| root.clone()).collect();
    let mut walked: Vec<PathBuf> = Vec::new();
    let mut failed: Option<Refused> = None;
    for ((_, root), reach) in &trees {
        let pool = reclaim.iter().find(|(r, _)| r == root).map(|(_, p)| p);
        let inside = walked.iter().any(|w| w != root && in_folder(root, w));
        if inside && pool.is_none() {
            walked.push(root.clone());
            continue;
        }
        let kind = PathKind::Matter;
        let done = match pool {
            Some(pool) => clearing(acl, around, &own, kind, |acl| acl.reclaim(root, pool)),
            None => clearing(acl, around, &own, kind, |acl| {
                acl.propagate(root, *reach, None)
            }),
        };
        match done {
            Ok(_) => walked.push(root.clone()),
            Err(refused) => {
                let failure = refused.failure;
                tracing::warn!(%failure, "a walk after a take-back failed; its entries stay pending");
                failed.get_or_insert(refused);
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
        .drop_gone(&ids, |entry| present_entry(acl, entry))
        .map_err(|e| Refused::new(PathKind::Matter, ApplyFailure::Ledger(e.kind())))?;
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
///
/// The hand-back it owes each pool user is recorded under the lock before any removal, and
/// cleared only once every hand-back walk has succeeded (N5): until then
/// [`Ledger::handback_owed`] holds the pool user back from reuse, and [`reconcile`] keeps the
/// matter's entries, so running this again finishes the job (an entry already removed is `Ok`).
/// A child the lawyer can't open inside the matter's own trees is set aside through `around`
/// and the hand-back walk run again. A hand-back that still fails (the child can't be set aside,
/// or the lawyer may not write it) refuses naming the child, and the hand-back stays owed.
pub fn forget_matter(
    ledger: &mut Ledger,
    matter: MatterId,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> Result<Forgotten, Error> {
    let failed = |e: io::Error| refused(PathKind::Matter, ApplyFailure::Ledger(e.kind()));
    // Read the file again, so the trees and pool users are the ones listed now.
    ledger.pending().map_err(failed)?;
    let entries = ledger.matter_entries(matter);
    let mut pool: BTreeSet<Sid> = entries
        .iter()
        .filter(|e| e.rights.is_allow())
        .map(|e| e.sid.clone())
        .collect();
    let owed = ledger.handbacks_owed().into_iter();
    pool.extend(owed.filter(|o| o.matter == matter).map(|o| o.pool));
    let pool: Vec<Sid> = pool.into_iter().collect();
    let reclaim: Vec<(PathBuf, Vec<Sid>)> = entries
        .iter()
        .filter(|e| {
            OWN_KINDS.contains(&e.kind) && e.rights.is_allow() && e.inherit == Inherit::Tree
        })
        .map(|e| e.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|path| (path, pool.clone()))
        .collect();
    let pick = |entry: &Entry| (entry.matter == matter).then_some(Take::Remove);
    let taken = take_back(ledger, Some(matter), acl, pick).map_err(failed)?;
    finish(ledger, acl, around, &taken, &reclaim).map_err(Refused::error)?;
    ledger.clear_handbacks(matter).map_err(failed)?;
    Ok(Forgotten {
        removed: taken.removed.len(),
        kept: taken.kept,
        shared: taken.shared,
    })
}

/// At start: walks each `pending` entry's tree again, plainly (an entry is `pending` when a
/// launch, a rollback or a forget stopped partway, the review's M2), then marks the ones whose
/// permission entry is there `applied` and drops the rest. An entry whose check or walk fails
/// stays `pending` for the next start. Two kinds stay `pending` on purpose, counted as
/// deferred: a deny entry whose permission entry is there, so its matter's next launch walks its
/// tree strictly, as a plain walk doesn't check what beats the deny (N4); and every entry of a
/// matter whose hand-back is owed, which a retried `forget_matter` needs (N5).
pub fn reconcile(ledger: &mut Ledger, acl: &mut dyn AclWriter) -> Result<Reconciled, Error> {
    let failed = |e: io::Error| refused(PathKind::Matter, ApplyFailure::Ledger(e.kind()));
    let pending = ledger.pending().map_err(failed)?;
    let owed: BTreeSet<MatterId> = ledger
        .handbacks_owed()
        .into_iter()
        .map(|o| o.matter)
        .collect();
    let mut done = Reconciled::default();
    let (mut there, mut gone) = (Vec::new(), Vec::new());
    for entry in pending {
        match present_entry(acl, &entry) {
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
    let deny = |entry: &&Entry| entry.rights.is_deny();
    let owes = |entry: &&Entry| owed.contains(&entry.matter);
    let applied: Vec<EntryId> = there
        .iter()
        .filter(|e| settled(e) && !deny(e))
        .map(|e| e.id)
        .collect();
    let dropped: Vec<EntryId> = gone
        .iter()
        .filter(|e| settled(e) && !owes(e))
        .map(|e| e.id)
        .collect();
    done.deferred = there.iter().filter(deny).count() + gone.iter().filter(owes).count();
    done.unchecked += there.len() + gone.len() - applied.len() - dropped.len() - done.deferred;
    ledger.mark_walked(&applied).map_err(failed)?;
    done.applied = applied.len();
    done.dropped = ledger
        .drop_gone(&dropped, |entry| present_entry(acl, entry))
        .map_err(failed)?;
    Ok(done)
}
