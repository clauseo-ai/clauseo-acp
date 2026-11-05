//! Applying a plan (S15 PR3): the write-ahead order of D4 around the permission writes, the walk
//! that carries inherited entries down each tree, and the way back.
//!
//! [`apply`] runs, in order:
//!
//! 1. A folder that is now a registered matter (this launch's, or one whose fence the plan
//!    carries) loses any sibling deny on it or above it first ([`lift_sibling_denies`]), for
//!    every matter whose ledger entry names that deny.
//! 2. Grants on names that don't exist are left out: NTFS holds no entry for a missing name, and
//!    a protected name the agent makes later is set aside before the next launch
//!    ([`set_aside_protected`](super::set_aside_protected)).
//! 3. Every grant is recorded `pending` ([`Ledger::record_pending`]), then written, path by path
//!    from the shallowest, and each path's entries marked `applied` once its writes are done.
//! 4. The walk ([`AclWriter::propagate`]) carries the inherited entries down: on every tree
//!    whose entry this launch wrote, and on the matter, state and notes folders at every launch,
//!    so a file moved in since the last launch picks them up. A walk over a tree that carries a
//!    deny is strict: a child whose DACL is protected, or that holds an explicit allow for a
//!    broad group or another pool subject, would beat the inherited deny, so the launch is
//!    refused.
//!
//! Any failure rolls back this launch's new entries only: each permission entry this launch
//! wrote for an entry it added is removed, unless another matter's entry names it
//! ([`Ledger::is_shared`]); the trees are walked again so the inherited copies go too; and the
//! entries are dropped. An entry whose removal fails is kept for [`forget_matter`] or the next
//! start's [`reconcile`].

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::acl_plan::{AdmittedPlan, Inherit, Rights, Sid, in_folder};
use super::admit::PathKind;
#[cfg(any(windows, test, feature = "test-support"))]
use super::fence::RegisteredMatter;
use super::ledger::{Entry, EntryId, Forgotten, Ledger, Reconciled};
use super::writer::{AclWriter, Added, ApplyFailure, Reach, Walked};
use crate::Error;

/// What [`apply`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applied {
    /// Entries written by this launch.
    pub written: usize,
    /// Sibling denies lifted because their folder is a registered matter now.
    pub lifted: usize,
    /// What the walks saw.
    pub walked: Walked,
}

/// Applies `plan` through `acl`, recorded in `ledger`: see the module's docs. A failure has
/// rolled back this launch's new entries when it returns ([`Error::AclApply`]).
pub fn apply(
    ledger: &mut Ledger,
    plan: &mut AdmittedPlan,
    acl: &mut dyn AclWriter,
) -> Result<Applied, Error> {
    let roots: Vec<PathBuf> = plan
        .grants()
        .iter()
        .filter(|g| g.kind() == PathKind::Matter)
        .map(|g| g.path().to_owned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let lifted = lift_sibling_denies(ledger, &roots, acl)?;
    plan.retain_existing(exists);
    ledger
        .record_pending(plan)
        .map_err(|e| refused(PathKind::Matter, ApplyFailure::Ledger(e.kind())))?;
    let mut run = Run::default();
    match write_all(ledger, plan, acl, &mut run) {
        Ok(()) => Ok(Applied {
            written: run.written.len(),
            lifted,
            walked: run.walked,
        }),
        Err((kind, failure)) => {
            tracing::warn!(%kind, %failure, "tier B's entries failed; rolling back");
            roll_back(ledger, plan, acl, &run);
            Err(refused(kind, failure))
        }
    }
}

/// What a run wrote, for its rollback.
#[derive(Debug, Default)]
struct Run {
    /// Indexes into the plan's grants whose entry this run wrote.
    written: Vec<usize>,
    walked: Walked,
}

fn write_all(
    ledger: &mut Ledger,
    plan: &AdmittedPlan,
    acl: &mut dyn AclWriter,
    run: &mut Run,
) -> Result<(), (PathKind, ApplyFailure)> {
    let grants = plan.grants();
    let mut by_path: BTreeMap<(usize, &Path), Vec<usize>> = BTreeMap::new();
    for (i, grant) in grants.iter().enumerate() {
        let depth = grant.path().components().count();
        by_path.entry((depth, grant.path())).or_default().push(i);
    }
    for indexes in by_path.values() {
        let mut kind = PathKind::Matter;
        for grant in indexes
            .iter()
            .filter_map(|&i| grants.get(i).map(|g| (i, g)))
        {
            let (i, grant) = grant;
            kind = grant.kind();
            match acl.add(grant).map_err(|f| (kind, f))? {
                Added::Written => run.written.push(i),
                Added::AlreadyThere => {}
            }
        }
        let ids: Vec<EntryId> = indexes
            .iter()
            .filter_map(|&i| plan.entries().get(i))
            .copied()
            .collect();
        ledger
            .mark_applied(plan, &ids)
            .map_err(|e| (kind, ApplyFailure::Ledger(e.kind())))?;
    }
    let watched: Vec<Sid> = grants
        .iter()
        .filter(|g| matches!(g.rights(), Rights::DenyReadWrite | Rights::DenyRead))
        .map(|g| g.sid().clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    for walk in walks(plan, |i| run.written.contains(&i)) {
        let strict = walk.strict.then_some(watched.as_slice());
        let seen = acl
            .propagate(&walk.root, walk.reach, strict)
            .map_err(|f| (walk.kind, f))?;
        run.walked.add(seen);
    }
    Ok(())
}

/// One walk to run.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Walk {
    root: PathBuf,
    reach: Reach,
    strict: bool,
    kind: PathKind,
}

/// The walks `plan` needs, shallowest first: each path with an inheritable grant whose entry
/// this run wrote (`written`), and the launch's own matter, state and notes folders always. A
/// tree inside another walked tree is left to that walk. A tree is strict when it is one of the
/// launch's own folders or carries a deny.
fn walks(plan: &AdmittedPlan, written: impl Fn(usize) -> bool) -> Vec<Walk> {
    let own_kinds = [PathKind::Matter, PathKind::State, PathKind::Notes];
    let mut found: BTreeMap<(usize, PathBuf), Walk> = BTreeMap::new();
    for (i, (grant, owner)) in plan.grants().iter().zip(plan.owners()).enumerate() {
        if grant.inherit() == Inherit::ThisOnly {
            continue;
        }
        let own = *owner == plan.matter()
            && grant.rights() == Rights::Modify
            && own_kinds.contains(&grant.kind());
        if !own && !written(i) {
            continue;
        }
        let reach = match grant.inherit() {
            Inherit::FilesInFolder => Reach::FilesOnly,
            Inherit::Tree | Inherit::ThisOnly => Reach::Tree,
        };
        let key = (grant.path().components().count(), grant.path().to_owned());
        let walk = found.entry(key).or_insert_with(|| Walk {
            root: grant.path().to_owned(),
            reach,
            strict: false,
            kind: grant.kind(),
        });
        walk.strict |= own || grant.rights().is_deny();
        if reach == Reach::Tree {
            walk.reach = Reach::Tree;
        }
    }
    let all: Vec<Walk> = found.into_values().collect();
    all.iter()
        .filter(|w| {
            !all.iter().any(|outer| {
                outer.reach == Reach::Tree
                    && outer.root != w.root
                    && in_folder(&w.root, &outer.root)
                    && (outer.strict || !w.strict)
            })
        })
        .cloned()
        .collect()
}

/// Removes the entries this run wrote for entries it added (never one another matter's entry
/// names), walks those trees again, and drops this run's entries. A removal or walk that fails
/// keeps its entries for later.
fn roll_back(ledger: &mut Ledger, plan: &mut AdmittedPlan, acl: &mut dyn AclWriter, run: &Run) {
    let created: Vec<EntryId> = plan.created().to_vec();
    let mut keep: BTreeSet<EntryId> = BTreeSet::new();
    let mut walk: BTreeMap<PathBuf, Reach> = BTreeMap::new();
    for &i in run.written.iter().rev() {
        let (Some(grant), Some(&id)) = (plan.grants().get(i), plan.entries().get(i)) else {
            continue;
        };
        if !created.contains(&id) {
            continue;
        }
        let shared = ledger
            .entries()
            .find(|e| e.id == id)
            .is_some_and(|e| ledger.is_shared(e));
        if shared {
            continue;
        }
        match acl.remove(grant) {
            Ok(()) if grant.inherit() != Inherit::ThisOnly => {
                let reach = if grant.inherit() == Inherit::FilesInFolder {
                    Reach::FilesOnly
                } else {
                    Reach::Tree
                };
                let slot = walk.entry(grant.path().to_owned()).or_insert(reach);
                if reach == Reach::Tree {
                    *slot = Reach::Tree;
                }
            }
            Ok(()) => {}
            Err(failure) => {
                tracing::warn!(kind = %grant.kind(), %failure, "a rollback removal failed");
                keep.insert(id);
            }
        }
    }
    for (root, reach) in &walk {
        if let Err(failure) = acl.propagate(root, *reach, None) {
            tracing::warn!(%failure, "a rollback walk failed; its entries are kept");
            for (grant, id) in plan.grants().iter().zip(plan.entries()) {
                if grant.path() == root {
                    keep.insert(*id);
                }
            }
        }
    }
    let drop: Vec<EntryId> = created
        .into_iter()
        .filter(|id| !keep.contains(id))
        .collect();
    if let Err(error) = ledger.drop_entries(plan, &drop) {
        let error = error.kind();
        tracing::warn!(%error, "a rollback couldn't drop its ledger entries");
    }
}

/// A folder that becomes a registered matter loses its sibling deny first: every sibling deny
/// (any matter's entry) on one of `roots` or on a folder holding one is removed, its tree
/// walked again, and every entry naming it dropped. The host's matter creation calls this too.
/// Returns how many entries were dropped.
pub fn lift_sibling_denies(
    ledger: &mut Ledger,
    roots: &[PathBuf],
    acl: &mut dyn AclWriter,
) -> Result<usize, Error> {
    let pick = |entry: &Entry| {
        entry.kind == PathKind::Sibling
            && entry.rights == Rights::DenyRead
            && roots.iter().any(|root| in_folder(root, &entry.path))
    };
    let remove = |entry: &Entry| -> io::Result<()> {
        acl.remove(&entry.grant())?;
        acl.propagate(&entry.path, Reach::Tree, None)?;
        Ok(())
    };
    ledger
        .lift(pick, remove)
        .map_err(|e| refused(PathKind::Sibling, ApplyFailure::Ledger(e.kind())))
}

/// Forgets `matter` without admin: removes each of its entries (labels included) unless
/// another matter's entry names it, walks each tree once its last entry is removed, and drops
/// the entries removed. An entry whose removal or walk failed is kept for the next try.
pub fn forget_matter(
    ledger: &mut Ledger,
    matter: MatterId,
    acl: &mut dyn AclWriter,
) -> Result<Forgotten, Error> {
    let mut left: BTreeMap<PathBuf, usize> = BTreeMap::new();
    for entry in ledger.matter_entries(matter) {
        if entry.inherit != Inherit::ThisOnly && !ledger.is_shared(&entry) {
            *left.entry(entry.path.clone()).or_default() += 1;
        }
    }
    let mut pending: BTreeMap<PathBuf, Reach> = BTreeMap::new();
    let done = ledger.forget_matter(matter, |entry| {
        acl.remove(&entry.grant())?;
        if entry.inherit == Inherit::ThisOnly {
            return Ok(());
        }
        let reach = if entry.inherit == Inherit::FilesInFolder {
            Reach::FilesOnly
        } else {
            Reach::Tree
        };
        let slot = pending.entry(entry.path.clone()).or_insert(reach);
        if reach == Reach::Tree {
            *slot = Reach::Tree;
        }
        let count = left.entry(entry.path.clone()).or_insert(1);
        *count = count.saturating_sub(1);
        if *count == 0 {
            let reach = pending.remove(&entry.path).unwrap_or(reach);
            acl.propagate(&entry.path, reach, None)?;
        }
        Ok(())
    });
    let done = done.map_err(|e| refused(PathKind::Matter, ApplyFailure::Ledger(e.kind())))?;
    // Paths whose count didn't reach zero (another process changed the ledger between the
    // count and the lock): walk them now.
    for (root, reach) in pending {
        acl.propagate(&root, reach, None)
            .map_err(|f| refused(PathKind::Matter, f))?;
    }
    Ok(done)
}

/// At start: keeps each pending entry whose permission entry is there and drops the rest
/// ([`Ledger::reconcile`]), asking `acl`.
pub fn reconcile(ledger: &mut Ledger, acl: &mut dyn AclWriter) -> Result<Reconciled, Error> {
    ledger
        .reconcile(|entry| acl.present(&entry.grant()))
        .map_err(|e| refused(PathKind::Matter, ApplyFailure::Ledger(e.kind())))
}

/// Fences every registered matter against the whole `pool`, as when the pool grows: each
/// matter's folders resolved and checked as the gate checks a neighbour's (`check` is the
/// folder check), then the fence planned and applied. Entries listed already are kept, so only
/// the new pool users' denies are written. PR4's set-up calls this after adding users.
#[cfg(any(windows, test, feature = "test-support"))]
pub fn fence_registered(
    ledger: &mut Ledger,
    registered: &[RegisteredMatter],
    pool: &[Sid],
    check: impl FnMut(&Path) -> io::Result<super::FolderSupport>,
    acl: &mut dyn AclWriter,
) -> Result<Applied, Error> {
    let fences = super::admit::fences_for(registered, check)?;
    let Some(mut plan) = AdmittedPlan::from_fences(&fences, pool) else {
        return Ok(Applied::default());
    };
    apply(ledger, &mut plan, acl)
}

/// Whether `path` is there (a link counts). A check that fails counts as there, so the write
/// meets the error.
fn exists(path: &Path) -> bool {
    !matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == io::ErrorKind::NotFound)
}

fn refused(path_kind: PathKind, failure: ApplyFailure) -> Error {
    Error::AclApply { path_kind, failure }
}

#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
#[cfg(test)]
mod tests;
