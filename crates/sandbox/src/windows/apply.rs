//! Applying a plan (S15 PR3): the write-ahead order of D4 around the permission writes, the walk
//! that carries inherited entries down each tree, and the way back.
//!
//! [`apply`] runs, in order:
//!
//! 1. Grants on names that don't exist are left out: NTFS holds no entry for a missing name, and
//!    a protected name the agent makes later is set aside after the session (`set_aside.rs`).
//! 2. The credential link must still be the expected credentials file (the same file ID), or the
//!    launch is refused. So is an expected file inside the state folder, or the link itself:
//!    the agent can write there (N8). The link's entry is written through
//!    [`AclWriter::add_expecting`], which compares the file ID on the handle it writes through.
//! 3. Every grant is recorded `pending` ([`Ledger::record_pending`]). Each new Low label entry
//!    first records the explicit label its path has now, so it can be put back, unless that
//!    label is our own: the one this grant writes, or the one another entry for the same file
//!    wrote (N7). Then the entries are written, path by path from the shallowest.
//! 4. The walk ([`AclWriter::propagate`]) carries the inherited entries down: on every tree whose
//!    entry this launch wrote or whose entry was still `pending`, and on the matter, state and
//!    notes folders at every launch, so a file moved in since the last launch picks them up. A
//!    walk over a tree that carries a deny is strict ([`Strict`]): a child a pool user owns that
//!    would beat the inherited deny is repaired, one the lawyer owns refuses the launch, and one
//!    the lawyer can't open is set aside ([`Surroundings::set_aside`]) if it is inside the
//!    launch's own matter, state or notes tree, and refuses it anywhere else (`walk.rs`). The
//!    protected names the walks of the matter's own tree met are kept for the set-aside (N1).
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
use super::acl_plan::{AdmittedPlan, Grant, Rights, in_folder};
use super::admit::{FolderList, PathKind};
#[cfg(any(windows, test, feature = "test-support"))]
use super::fence::RegisteredMatter;
use super::ledger::{EntryId, EntryState, Ledger, is_our_label};
use super::writer::{AclWriter, Added, ApplyFailure, Found, Label, Reach, Walked};
use crate::Error;

mod back;
mod lift;
mod walk;

pub use back::{forget_matter, reconcile};
use walk::{OWN_KINDS, Refused, Walk, clearing, inside_any, own_roots, roots_of, strict_for};
use walk::{strict_of, walks};

/// What [`apply`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// Entries written by this launch.
    pub written: usize,
    /// Sibling deny entries lifted, after the launch succeeded, because their folder is a
    /// registered matter now or holds one.
    pub lifted: usize,
    /// What the walks saw.
    pub walked: Walked,
    /// The protected names (`.git`, `.claude`, `.mcp.json` at any depth) this launch's walks
    /// of its own matter tree met, with their file IDs. PR6 passes them to the set-aside's
    /// `before_launch`, which records them so `after_session` can set aside what changed (N1).
    pub found: Vec<Found>,
}

/// What [`repair_after_session`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Repaired {
    /// What the walks saw.
    pub walked: Walked,
    /// The protected names the walks of the matter's own tree met, with their file IDs. PR6
    /// passes them to the set-aside's `after_session`, which sets aside what is new or changed
    /// since launch (N1).
    pub found: Vec<Found>,
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
        found: run.found.into_iter().collect(),
    })
}

/// What a run wrote, for its rollback, and what its walks saw.
#[derive(Debug, Default)]
struct Run {
    /// Indexes into the plan's grants whose entry this run wrote.
    written: Vec<usize>,
    walked: Walked,
    /// The protected names met inside the plan's own matter folder.
    found: BTreeSet<Found>,
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
        Err(mut refused) => {
            refused.at = refused.at.or_else(|| acl.refused_at());
            let (path_kind, failure) = (refused.kind, refused.failure);
            tracing::warn!(%path_kind, %failure, "tier B's entries failed; rolling back");
            back::roll_back(ledger, plan, acl, around, &run);
            Err(refused.error())
        }
    }
}

/// L5: the credential link is the file the launcher expects, by its file ID. The link sits in
/// the state folder, which the agent can write; the entry and Low label it gets must not land on
/// another file. So the expected file must not be in the state folder itself, nor be the link
/// (N8): either would make the check compare the link with itself.
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
    let states: Vec<PathBuf> = plan
        .grants()
        .iter()
        .filter(|g| g.kind() == PathKind::State)
        .map(|g| g.path().to_owned())
        .collect();
    let canonical = expected
        .parent()
        .and_then(|folder| dunce::canonicalize(folder).ok())
        .zip(expected.file_name())
        .map(|(folder, name)| folder.join(name));
    let agents = |p: &PathBuf| in_folder(p, link.path()) || inside_any(p, &states);
    if std::iter::once(&expected).chain(&canonical).any(agents) {
        return Err(refused(kind, ApplyFailure::Changed));
    }
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
) -> Result<(), Refused> {
    let grants = plan.grants();
    let ledger_failed = |kind, e: io::Error| Refused::new(kind, ApplyFailure::Ledger(e.kind()));
    // L4: a new label entry first records the explicit label there now, before ours replaces it.
    for (grant, &id) in grants.iter().zip(plan.entries()) {
        if grant.rights().is_label() && plan.created().contains(&id) {
            let found = acl.label(grant.path());
            let found =
                found.map_err(|e| Refused::new(grant.kind(), ApplyFailure::Io(e.kind())))?;
            if let Some(label) = found
                && !ours(ledger, acl, grant, id, &label)
            {
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
            let added = match (grant.kind(), around.credentials()) {
                (PathKind::CredentialLink, Some(expected)) => acl.add_expecting(grant, &expected),
                (PathKind::CredentialLink, None) => Err(ApplyFailure::Changed),
                _ => acl.add(grant),
            };
            match added.map_err(|f| Refused::new(grant.kind(), f))? {
                Added::Written => run.written.push(i),
                Added::AlreadyThere => {}
            }
        }
    }
    let strict = strict_for(plan);
    let own = own_roots(plan);
    let matter = roots_of(plan, &[PathKind::Matter]);
    let unwalked = |i: usize| {
        run.written.contains(&i) || plan.entries().get(i).is_some_and(|id| pending.contains(id))
    };
    // N1: nothing an earlier walk met comes in.
    drop(acl.found());
    for walk in walks(plan, unwalked) {
        let Walk {
            root,
            reach,
            strict: is_strict,
            kind,
        } = &walk;
        let strict = is_strict.then_some(&strict);
        let seen = clearing(acl, around, &own, *kind, |acl| {
            acl.propagate(root, *reach, strict)
        })?;
        run.walked.add(seen);
        let met = acl.found().into_iter();
        run.found
            .extend(met.filter(|found| inside_any(&found.path, &matter)));
    }
    ledger
        .mark_walked(plan.entries())
        .map_err(|e| ledger_failed(PathKind::Matter, e))
}

/// Whether `label`, found on label grant `grant`'s path (entry `id`), is our own: the label
/// this grant writes, or the one another ledger entry for the same file wrote. Never recorded as
/// the lawyer's earlier label (N7).
fn ours(
    ledger: &Ledger,
    acl: &mut dyn AclWriter,
    grant: &Grant,
    id: EntryId,
    label: &Label,
) -> bool {
    if is_our_label(label, grant.inherit()) {
        return true;
    }
    ledger.entries().any(|e| {
        e.id != id
            && e.rights.is_label()
            && is_our_label(label, e.inherit)
            && (e.path == grant.path() || acl.same_file(&e.path, grant.path()).unwrap_or(false))
    })
}

/// When `matter`'s agent exits (PR6's launcher calls this beside the set-aside's
/// `after_session`): its matter, state and notes trees walked strictly again, so what the agent
/// made open to other matters' agents during the session is repaired now rather than at the next
/// launch (the review's M5). Children the lawyer can't open in those trees are set aside as at
/// launch. Returns the protected names the walks of the matter's own tree met, for the
/// set-aside's `after_session` (N1).
pub fn repair_after_session(
    ledger: &Ledger,
    matter: clauseo_protocol::MatterId,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> Result<Repaired, Error> {
    let entries = ledger.matter_entries(matter);
    let strict = strict_of(&entries);
    let roots: BTreeSet<(PathBuf, PathKind)> = entries
        .iter()
        .filter(|e| OWN_KINDS.contains(&e.kind) && e.rights == Rights::Modify)
        .map(|e| (e.path.clone(), e.kind))
        .collect();
    let own: Vec<PathBuf> = roots.iter().map(|(root, _)| root.clone()).collect();
    let matter_roots: Vec<PathBuf> = roots
        .iter()
        .filter(|(_, kind)| *kind == PathKind::Matter)
        .map(|(root, _)| root.clone())
        .collect();
    let mut done = Repaired::default();
    let mut found: BTreeSet<Found> = BTreeSet::new();
    drop(acl.found());
    for (root, kind) in &roots {
        let walked = clearing(acl, around, &own, *kind, |acl| {
            acl.propagate(root, Reach::Tree, Some(&strict))
        })
        .map_err(Refused::error)?;
        done.walked.add(walked);
        let met = acl.found().into_iter();
        found.extend(met.filter(|f| inside_any(&f.path, &matter_roots)));
    }
    done.found = found.into_iter().collect();
    Ok(done)
}

/// Fences every registered matter against the whole `pool`, as when the pool grows: each
/// matter's folders resolved and checked as the gate checks a neighbour's (`around`'s folder
/// check), then the fence planned and applied. Entries listed already are kept, so only the new
/// pool users' denies are written. PR4's set-up calls this after adding users.
#[cfg(any(windows, test, feature = "test-support"))]
pub fn fence_registered(
    ledger: &mut Ledger,
    registered: &[RegisteredMatter],
    pool: &[super::acl_plan::Sid],
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
