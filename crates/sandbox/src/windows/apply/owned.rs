//! Step 3b of [`apply`](super::apply): each folder Clauseo owns for the launch
//! ([`AdmittedPlan::owned_roots`]: the data folder, its `agent-state`, `run` and `agents`
//! folders, the matter's, the state, notes, run and temp folders) gets its protected DACL, after
//! the roots' hand-back and before anything is recorded. Nothing is recorded for it, and a
//! rollback leaves it: the folder is Clauseo's, and a protected DACL with the lawyer's and
//! `SYSTEM`'s full access is where it belongs whatever the launch does. Each explicit allow the
//! plan or the ledger has there stays, so another live session keeps its access (the review's L3).
//!
//! In step 5, each one whose DACL changed, or that the hand-back took from a pool user (its DACL
//! may have matched already, the review's N6), is gone over again ([`OwnedWork`]), but the data
//! folder's two shared folders: an owner pass first, so an item a pool user made there before
//! Clauseo protected it (a planted file or junction in a run folder another matter's agent made
//! first) goes to the lawyer with its `WRITE_DAC` (L1), then a walk, so what is in it loses the
//! entries it inherited from above (`C:\`'s `Users` entries).
//!
//! The data folder's contents are walked once: `agents` strictly, even when the data folder
//! itself is skipped (N2), right after the protection and before anything is recorded
//! ([`walk_sign_ins`]), a failure refusing the launch before any grant is written, so no sign-in
//! stays readable to other accounts (N1, [`Error::SignInUnsecured`]); the host's other folders
//! plainly with the launch's walks ([`walk_data`]), a failure only logged. Once, that is: when
//! the data folder's or `agents`' DACL changes, and at each launch until one such walk has passed
//! (`sandbox\owned-walked`). The
//! strict walk watches nothing, so it strips only broad allows (`Users`, `Everyone`) from what a
//! pool user owns there: another live session's recorded allows in the Claude store stay (N7).

use std::io;
use std::path::{Path, PathBuf};

use super::walk::Walk;
use super::{exists, strict_for};
use crate::Error;
use crate::windows::acl_plan::{AdmittedPlan, Grant, OwnedKind, Sid, in_folder};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{EntryState, Ledger};
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::write;
use crate::windows::writer::{AclWriter, Added, Reach, Strict, Walked};

/// The data folder's children its plain walk leaves out: every matter's and session's folders
/// (each protected and walked at its own launch), the sandbox's records (protected) and `agents`
/// (walked strictly on its own).
const NOT_WALKED: [&str; 4] = ["agent-state", "run", SANDBOX_DIR, "agents"];

/// The record that a data walk passed, in `<data_dir>\sandbox`.
const WALKED_MARK: &str = "owned-walked";

/// What step 5 does for the owned roots.
#[derive(Debug, Default)]
pub(super) struct OwnedWork {
    /// Owner passes, outermost first and none inside another with the same SIDs: each root and
    /// the pool SIDs whose items there go to the lawyer.
    pub(super) passes: Vec<(PathBuf, PathKind, Vec<Sid>)>,
    /// Walks, with the launch's own (strict for the state and notes folders).
    pub(super) walks: Vec<Walk>,
    /// The data walk this launch, when one is due.
    pub(super) data: Option<DataWalk>,
}

/// The data folder's walk ([`walk_data`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DataWalk {
    /// `<data_dir>`, where the record goes.
    data: PathBuf,
    /// Whether the data folder is owned, so its own items and other folders are walked too.
    whole: bool,
    /// `<data_dir>\agents`, when it is there: walked strictly.
    agents: Option<PathBuf>,
    /// The pool SIDs the strict walk holds `agents` to.
    pool: Vec<Sid>,
}

/// Protects each of `plan`'s owned roots through `acl`, keeping the explicit denies for the pool
/// SIDs `plan` names and `pool`, and every allow the plan or `ledger` has there; what step 5 does
/// for them, `handed` being the roots the hand-back took from a pool user. The first refusal
/// refuses the launch, naming the root.
pub(super) fn protect_owned(
    plan: &AdmittedPlan,
    ledger: &Ledger,
    acl: &mut dyn AclWriter,
    pool: &[Sid],
    handed: &[PathBuf],
) -> Result<OwnedWork, Error> {
    let mut sids = strict_for(plan).pool;
    sids.extend(pool.iter().cloned());
    sids.sort();
    sids.dedup();
    let own = plan.assigned_to(plan.matter());
    let mut work = OwnedWork::default();
    let (mut data, mut whole, mut agents, mut changed) = (None, false, None, false);
    for root in plan.owned_roots() {
        let keep = kept(plan, ledger, root.path());
        let added = acl
            .protect(root, &sids, &keep)
            .map_err(|failure| Error::AclApply {
                path_kind: root.path_kind(),
                failure,
                at: Some(root.path().to_owned()),
            })?;
        let fresh = added == Added::Written || handed.iter().any(|h| h == root.path());
        match root.kind() {
            OwnedKind::DataRoot => {
                (data, whole) = (Some(root.path().to_owned()), true);
                changed |= fresh;
            }
            OwnedKind::Agents => {
                agents = Some(root.path().to_owned());
                data = data.or_else(|| root.path().parent().map(Path::to_owned));
                changed |= fresh;
            }
            OwnedKind::DataFolder
            | OwnedKind::MatterFolder
            | OwnedKind::State
            | OwnedKind::Notes
            | OwnedKind::RunFolder
            | OwnedKind::TempFolder => {}
        }
        if !fresh || !root.walked() {
            continue;
        }
        let foreign: Vec<Sid> = if root.agents_own() {
            sids.iter().filter(|s| !own.contains(s)).cloned().collect()
        } else {
            sids.clone()
        };
        let covered = work
            .passes
            .iter()
            .any(|(outer, _, them)| in_folder(root.path(), outer) && *them == foreign);
        if !covered {
            work.passes
                .push((root.path().to_owned(), root.path_kind(), foreign));
        }
        work.walks.push(Walk {
            root: root.path().to_owned(),
            reach: Reach::Tree,
            strict: matches!(root.kind(), OwnedKind::State | OwnedKind::Notes),
            kind: root.path_kind(),
        });
    }
    if let Some(data) = data
        && (changed || !exists(&mark(&data)))
    {
        let pool = sids;
        work.data = Some(DataWalk {
            data,
            whole,
            agents,
            pool,
        });
    }
    if !work.walks.is_empty() || work.data.is_some() {
        tracing::info!(
            roots = work.walks.len(),
            data = work.data.is_some(),
            "owned folders protected; their trees are gone over"
        );
    }
    Ok(work)
}

/// The allows `root` keeps: the plan's there, and each one `ledger` records there for any
/// matter, applied or pending (a live session's), with its own mask and inheritance.
fn kept(plan: &AdmittedPlan, ledger: &Ledger, root: &Path) -> Vec<Grant> {
    let same = |p: &Path| in_folder(p, root) && in_folder(root, p);
    let planned = plan
        .grants()
        .iter()
        .filter(|g| g.rights().is_allow() && same(g.path()))
        .cloned();
    let recorded = ledger
        .entries()
        .filter(|e| matches!(e.state, EntryState::Applied | EntryState::Pending))
        .filter(|e| e.rights.is_allow() && same(&e.path))
        .map(super::super::ledger::Entry::grant);
    planned.chain(recorded).collect()
}

/// `<data_dir>\sandbox\owned-walked` for the data folder `data`.
fn mark(data: &Path) -> PathBuf {
    data.join(SANDBOX_DIR).join(WALKED_MARK)
}

/// The first half of the data walk `walk`, before anything is recorded: `agents` walked
/// strictly (held to the pool SIDs, nothing watched: a pool user's item there is repaired, other
/// allows stay). A failure refuses the launch as [`Error::SignInUnsecured`] before any grant is
/// written or recorded.
pub(super) fn walk_sign_ins(acl: &mut dyn AclWriter, walk: &DataWalk) -> Result<Walked, Error> {
    let Some(agents) = &walk.agents else {
        return Ok(Walked::default());
    };
    let strict = Strict {
        watched: Vec::new(),
        pool: walk.pool.clone(),
        assigned: Vec::new(),
    };
    acl.propagate(agents, Reach::Tree, Some(&strict))
        .map_err(|failure| {
            tracing::warn!(%failure, "the sign-ins' folder wasn't secured; launch refused");
            Error::SignInUnsecured { failure }
        })
}

/// The second half of the data walk `walk`, once `agents` has passed ([`walk_sign_ins`]): when
/// the data folder is owned, each item directly in it worked out from it and each folder in it
/// but [`NOT_WALKED`] walked down, a failure only logged. Then, when nothing failed, the record
/// that the data walk passed.
pub(super) fn walk_data(acl: &mut dyn AclWriter, walk: &DataWalk) -> Walked {
    let mut seen = Walked::default();
    let mut passed = true;
    if walk.whole {
        let mut walk_one = |root: &Path, reach| match acl.propagate(root, reach, None) {
            Ok(walked) => seen.add(walked),
            Err(failure) => {
                tracing::warn!(%failure, "a data folder's item wasn't walked; it runs again");
                passed = false;
            }
        };
        walk_one(&walk.data, Reach::Children);
        for child in data_children(&walk.data) {
            walk_one(&child, Reach::Tree);
        }
    }
    if passed && let Err(error) = record_walked(&walk.data) {
        tracing::warn!(%error, "the data walk's record wasn't written; it runs again");
    }
    seen
}

/// The folders directly in `data` its plain walk goes down: none of [`NOT_WALKED`], no link.
/// Sorted, so walks run in one order.
fn data_children(data: &Path) -> Vec<PathBuf> {
    let Ok(listing) = std::fs::read_dir(data) else {
        tracing::warn!("the data folder couldn't be listed for its walk");
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = listing
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|t| t.is_dir() && !t.is_symlink())
        })
        .filter(|entry| {
            let name = entry.file_name();
            !NOT_WALKED.iter().any(|n| name.eq_ignore_ascii_case(n))
        })
        .map(|entry| entry.path())
        .collect();
    out.sort();
    out
}

/// Writes the record that a data walk passed.
fn record_walked(data: &Path) -> io::Result<()> {
    let path = mark(data);
    if let Some(sandbox) = path.parent() {
        write::make_sandbox_dir(sandbox)?;
    }
    write::check_owned_file(&path)?;
    write::write_atomic(&path, b"the data folder's contents were walked once\n")?;
    write::check_owned_file(&path)
}
