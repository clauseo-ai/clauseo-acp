//! Applying a plan (S15 PR3): the write-ahead order of D4 around the permission writes, the walk
//! that carries inherited entries down each tree, and the way back.
//!
//! [`apply`] runs, in order:
//!
//! 1. Grants on names that don't exist are left out: NTFS holds no entry for a missing name, and
//!    a protected name the agent makes later is set aside after the session (`set_aside.rs`).
//! 2. The credential link must still be the expected credentials file (the same file ID), or the
//!    launch is refused.
//! 3. Every grant is recorded `pending` ([`Ledger::record_pending`]). Each new Low label entry
//!    first records the explicit label its path has now, so it can be put back. Then the entries
//!    are written, path by path from the shallowest.
//! 4. The walk ([`AclWriter::propagate`]) carries the inherited entries down: on every tree whose
//!    entry this launch wrote or whose entry was still `pending`, and on the matter, state and
//!    notes folders at every launch, so a file moved in since the last launch picks them up. A
//!    walk over a tree that carries a deny is strict ([`Strict`]): a child a pool user owns that
//!    would beat the inherited deny is repaired, one the lawyer owns refuses the launch, and one
//!    the lawyer can't open is set aside ([`Surroundings::set_aside`]) or refuses it.
//! 5. Only then are the entries marked `applied`.
//! 6. Only after all that succeeds does a folder that is now a registered matter lose the
//!    sibling deny on it, for every matter whose entry names it; a deny on a folder holding it is
//!    replaced by the walk-down set first (`lift.rs`). A refused launch leaves every deny.
//!
//! Any failure rolls back this launch's new entries only (`back.rs`): under the ledger's lock
//! they are set `pending`, each permission entry this launch wrote for them is removed unless
//! another entry names it, the trees are walked again so the inherited copies go too, and the
//! entries whose permission entry is gone are dropped. An entry whose removal fails stays
//! `pending` for [`forget_matter`] or the next start's [`reconcile`].

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use super::FolderSupport;
use super::acl_plan::{AdmittedPlan, Inherit, Rights, Sid, in_folder};
use super::admit::{FolderList, PathKind};
#[cfg(any(windows, test, feature = "test-support"))]
use super::fence::RegisteredMatter;
use super::ledger::{EntryId, EntryState, Ledger};
use super::writer::{AclWriter, Added, ApplyFailure, Reach, Strict, Walked};
use crate::Error;

mod back;
mod lift;

pub use back::{forget_matter, reconcile};

/// What [`apply`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applied {
    /// Entries written by this launch.
    pub written: usize,
    /// Sibling deny entries lifted, after the launch succeeded, because their folder is a
    /// registered matter now or holds one.
    pub lifted: usize,
    /// What the walks saw.
    pub walked: Walked,
}

/// What a launch needs from around the matter besides the permission writes: the gate's folder
/// listing and folder check (to replace a deny on a folder holding the matter by the walk-down
/// set), the set-aside of a child the lawyer can't open, and the credentials file the
/// credential link must be. PR6's launcher gives `admit::ReadDir`, `folder_support`, the
/// set-aside under `<data_dir>\sandbox\set-aside\<matter>\` and the agent's credentials file.
pub trait Surroundings: FolderList {
    /// The gate's folder check.
    fn check(&self, folder: &Path) -> io::Result<FolderSupport>;
    /// Moves `path`, a child the lawyer can't open, aside (through its folder's
    /// `FILE_DELETE_CHILD`), with a manifest entry the lawyer can restore from.
    fn set_aside(&mut self, path: &Path) -> io::Result<()>;
    /// The file the credential link must be (the same file, by its ID), when the plan has one.
    fn credentials(&self) -> Option<PathBuf>;
}

/// Applies `plan` through `acl`, recorded in `ledger`: see the module's docs. A failure has
/// rolled back this launch's new entries when it returns ([`Error::AclApply`]).
pub fn apply(
    ledger: &mut Ledger,
    plan: &mut AdmittedPlan,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> Result<Applied, Error> {
    let roots: Vec<PathBuf> = plan
        .grants()
        .iter()
        .filter(|g| g.kind() == PathKind::Matter)
        .map(|g| g.path().to_owned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let run = run_plan(ledger, plan, acl, around)?;
    let lifted = lift::lift(ledger, plan, &roots, acl, around);
    Ok(Applied {
        written: run.written.len(),
        lifted,
        walked: run.walked,
    })
}

/// What a run wrote, for its rollback.
#[derive(Debug, Default)]
struct Run {
    /// Indexes into the plan's grants whose entry this run wrote.
    written: Vec<usize>,
    walked: Walked,
}

/// Steps 1 to 5 of [`apply`], rolled back on a failure.
fn run_plan(
    ledger: &mut Ledger,
    plan: &mut AdmittedPlan,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> Result<Run, Error> {
    plan.retain_existing(exists);
    check_credentials(plan, acl, &*around)?;
    ledger
        .record_pending(plan)
        .map_err(|e| refused(PathKind::Matter, ApplyFailure::Ledger(e.kind())))?;
    let mut run = Run::default();
    match write_all(ledger, plan, acl, around, &mut run) {
        Ok(()) => Ok(run),
        Err((path_kind, failure)) => {
            let at = acl.refused_at();
            tracing::warn!(%path_kind, %failure, "tier B's entries failed; rolling back");
            back::roll_back(ledger, plan, acl, &run);
            Err(Error::AclApply {
                path_kind,
                failure,
                at,
            })
        }
    }
}

/// L5: the credential link is the file the launcher expects, by its file ID. The link sits in
/// the state folder, which the agent can write; the entry and Low label it gets must not land on
/// another file.
fn check_credentials(
    plan: &AdmittedPlan,
    acl: &mut dyn AclWriter,
    around: &dyn Surroundings,
) -> Result<(), Error> {
    let kind = PathKind::CredentialLink;
    let Some(link) = plan.grants().iter().find(|g| g.kind() == kind) else {
        return Ok(());
    };
    let Some(expected) = around.credentials() else {
        return Err(refused(kind, ApplyFailure::Changed));
    };
    match acl.same_file(link.path(), &expected) {
        Ok(true) => Ok(()),
        Ok(false) => Err(refused(kind, ApplyFailure::Changed)),
        Err(error) => Err(refused(kind, ApplyFailure::Io(error.kind()))),
    }
}

fn write_all(
    ledger: &mut Ledger,
    plan: &AdmittedPlan,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
    run: &mut Run,
) -> Result<(), (PathKind, ApplyFailure)> {
    let grants = plan.grants();
    let ledger_failed = |kind, e: io::Error| (kind, ApplyFailure::Ledger(e.kind()));
    // L4: a new label entry first records the explicit label there now, before ours replaces it.
    for (grant, &id) in grants.iter().zip(plan.entries()) {
        if grant.rights().is_label() && plan.created().contains(&id) {
            let found = acl.label(grant.path());
            let found = found.map_err(|e| (grant.kind(), ApplyFailure::Io(e.kind())))?;
            if let Some(label) = found {
                ledger
                    .set_prior_label(plan, id, label)
                    .map_err(|e| ledger_failed(grant.kind(), e))?;
            }
        }
    }
    // M2: an entry still pending (this launch's, or one an earlier launch left) gets its walk.
    let pending: BTreeSet<EntryId> = ledger
        .entries()
        .filter(|e| e.state == EntryState::Pending && plan.entries().contains(&e.id))
        .map(|e| e.id)
        .collect();
    let mut by_path: BTreeMap<(usize, &Path), Vec<usize>> = BTreeMap::new();
    for (i, grant) in grants.iter().enumerate() {
        let depth = grant.path().components().count();
        by_path.entry((depth, grant.path())).or_default().push(i);
    }
    for indexes in by_path.values() {
        for (i, grant) in indexes
            .iter()
            .filter_map(|&i| grants.get(i).map(|g| (i, g)))
        {
            match acl.add(grant).map_err(|f| (grant.kind(), f))? {
                Added::Written => run.written.push(i),
                Added::AlreadyThere => {}
            }
        }
    }
    let strict = strict_for(plan);
    let unwalked = |i: usize| {
        run.written.contains(&i) || plan.entries().get(i).is_some_and(|id| pending.contains(id))
    };
    for walk in walks(plan, unwalked) {
        let seen = walk_with_set_aside(acl, around, &walk, walk.strict.then_some(&strict))?;
        run.walked.add(seen);
    }
    ledger
        .mark_walked(plan.entries())
        .map_err(|e| ledger_failed(PathKind::Matter, e))
}

/// One walk; a child the lawyer can't open is set aside and the walk run once more.
fn walk_with_set_aside(
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
    walk: &Walk,
    strict: Option<&Strict>,
) -> Result<Walked, (PathKind, ApplyFailure)> {
    match acl.propagate(&walk.root, walk.reach, strict) {
        Err(ApplyFailure::ChildShut) => {
            let Some(shut) = acl.refused_at() else {
                return Err((walk.kind, ApplyFailure::ChildShut));
            };
            if let Err(error) = around.set_aside(&shut) {
                let error = error.kind();
                tracing::warn!(%error, "a child the lawyer can't open couldn't be set aside");
                // Keep the path for the refusal's cause.
                return Err((walk.kind, keep_refused(acl, walk, strict)));
            }
            acl.propagate(&walk.root, walk.reach, strict)
                .map_err(|f| (walk.kind, f))
        }
        other => other.map_err(|f| (walk.kind, f)),
    }
}

/// Walks again so the writer holds the shut child's path for the refusal, and gives the
/// failure: [`ApplyFailure::ChildShut`] unless the walk now fails otherwise.
fn keep_refused(acl: &mut dyn AclWriter, walk: &Walk, strict: Option<&Strict>) -> ApplyFailure {
    match acl.propagate(&walk.root, walk.reach, strict) {
        Ok(_) => ApplyFailure::ChildShut,
        Err(failure) => failure,
    }
}

/// What a strict walk of `plan`'s trees holds children to: the SIDs its denies name, and every
/// pool SID it names (the owners whose children are repaired).
fn strict_for(plan: &AdmittedPlan) -> Strict {
    let sids = |keep: &dyn Fn(Rights) -> bool| -> Vec<Sid> {
        plan.grants()
            .iter()
            .filter(|g| keep(g.rights()))
            .map(|g| g.sid().clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    Strict {
        watched: sids(&|r| matches!(r, Rights::DenyReadWrite | Rights::DenyRead)),
        pool: sids(&|r| !r.is_label()),
    }
}

/// One walk to run.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Walk {
    root: PathBuf,
    reach: Reach,
    strict: bool,
    kind: PathKind,
}

/// The walks `plan` needs, shallowest first: each path with an inheritable grant `unwalked`
/// names (this run wrote its entry, or its entry was still pending), and the launch's own
/// matter, state and notes folders always. A tree inside another walked tree is left to that
/// walk. A tree is strict when it is one of the launch's own folders or carries a deny.
fn walks(plan: &AdmittedPlan, unwalked: impl Fn(usize) -> bool) -> Vec<Walk> {
    let own_kinds = [PathKind::Matter, PathKind::State, PathKind::Notes];
    let mut found: BTreeMap<(usize, PathBuf), Walk> = BTreeMap::new();
    for (i, (grant, owner)) in plan.grants().iter().zip(plan.owners()).enumerate() {
        if grant.inherit() == Inherit::ThisOnly {
            continue;
        }
        let own = *owner == plan.matter()
            && grant.rights() == Rights::Modify
            && own_kinds.contains(&grant.kind());
        if !own && !unwalked(i) {
            continue;
        }
        let reach = reach_of(grant.inherit());
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

/// How far an entry's inheritance reaches.
fn reach_of(inherit: Inherit) -> Reach {
    match inherit {
        Inherit::FilesInFolder => Reach::FilesOnly,
        Inherit::Tree | Inherit::ThisOnly => Reach::Tree,
    }
}

/// Fences every registered matter against the whole `pool`, as when the pool grows: each
/// matter's folders resolved and checked as the gate checks a neighbour's (`around`'s folder
/// check), then the fence planned and applied. Entries listed already are kept, so only the new
/// pool users' denies are written. PR4's set-up calls this after adding users.
#[cfg(any(windows, test, feature = "test-support"))]
pub fn fence_registered(
    ledger: &mut Ledger,
    registered: &[RegisteredMatter],
    pool: &[Sid],
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> Result<Applied, Error> {
    let fences = super::admit::fences_for(registered, |p| around.check(p))?;
    let Some(mut plan) = AdmittedPlan::from_fences(&fences, pool) else {
        return Ok(Applied::default());
    };
    apply(ledger, &mut plan, acl, around)
}

/// Whether `path` is there (a link counts). A check that fails counts as there, so the write
/// meets the error.
fn exists(path: &Path) -> bool {
    !matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == io::ErrorKind::NotFound)
}

fn refused(path_kind: PathKind, failure: ApplyFailure) -> Error {
    Error::AclApply {
        path_kind,
        failure,
        at: None,
    }
}

#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
#[cfg(test)]
mod tests;
