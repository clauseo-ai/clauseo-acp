//! Step 3b of [`apply`](super::apply): each folder Clauseo owns for the launch
//! ([`AdmittedPlan::owned_roots`]: the data folder, its `agent-state` and `run` folders, the
//! matter's, the state, notes, run and temp folders) gets its protected DACL, after the roots'
//! hand-back and before anything is recorded. Nothing is recorded for it, and a rollback leaves
//! it: the folder is Clauseo's, and a protected DACL with the lawyer's and `SYSTEM`'s full access
//! is where it belongs whatever the launch does. Each explicit allow the plan or the ledger has
//! there stays, so another live session of the matter keeps its access (the review's L3).
//!
//! In step 5, each one whose DACL changed, but the data folder's two shared folders, is gone
//! over again ([`OwnedWork`]): an owner pass first, so an item a pool user made there before
//! Clauseo protected it (a planted file or junction in a run folder another matter's agent made
//! first) goes to the lawyer with its `WRITE_DAC` (the review's L1), then a walk, so what is in
//! it loses the entries it inherited from above (`C:\`'s `Users` entries). The data folder's
//! other contents (`agents`, with Clauseo's Claude sign-in, the host's files) are walked once
//! too, strictly where credentials are (the review's L2): after the data folder's DACL changes,
//! and at each launch until one such walk has passed (`sandbox\owned-walked`); a refusal there
//! is logged and the launch goes on.

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

/// The data folder's children no data walk goes into: every matter's and session's folders
/// (each protected and walked at its own launch) and the sandbox's records (protected).
const NOT_WALKED: [&str; 3] = ["agent-state", "run", SANDBOX_DIR];

/// The data folder's child that holds credentials (each agent's install and home, Clauseo's
/// Claude sign-in among them): walked strictly.
const CREDENTIALS: &str = "agents";

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
    /// The data folder, when its other contents are walked this launch.
    pub(super) data: Option<PathBuf>,
}

/// Protects each of `plan`'s owned roots through `acl`, keeping the explicit denies for the pool
/// SIDs `plan` names and `pool`, and every allow the plan or `ledger` has there; what step 5 does
/// for them. The first refusal refuses the launch, naming the root.
pub(super) fn protect_owned(
    plan: &AdmittedPlan,
    ledger: &Ledger,
    acl: &mut dyn AclWriter,
    pool: &[Sid],
) -> Result<OwnedWork, Error> {
    let mut sids = strict_for(plan).pool;
    sids.extend(pool.iter().cloned());
    sids.sort();
    sids.dedup();
    let own = plan.assigned_to(plan.matter());
    let mut work = OwnedWork::default();
    for root in plan.owned_roots() {
        let keep = kept(plan, ledger, root.path());
        let added = acl
            .protect(root, &sids, &keep)
            .map_err(|failure| Error::AclApply {
                path_kind: root.path_kind(),
                failure,
                at: Some(root.path().to_owned()),
            })?;
        let data_root = root.kind() == OwnedKind::DataRoot;
        if data_root && (added == Added::Written || !exists(&mark(root.path()))) {
            work.data = Some(root.path().to_owned());
        }
        if added == Added::AlreadyThere || !root.walked() {
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

/// The data folder's walk: each item directly in `data` worked out from it, then each folder in
/// it but [`NOT_WALKED`] walked down, [`CREDENTIALS`] strictly (held to `pool`, nothing
/// watched: a pool user's item there is repaired). Then the record that it passed. A refusal is
/// logged and leaves no record, so the next launch walks again; the launch goes on.
pub(super) fn walk_data(acl: &mut dyn AclWriter, data: &Path, pool: &[Sid]) -> Walked {
    let mut seen = Walked::default();
    let strict = Strict {
        watched: Vec::new(),
        pool: pool.to_vec(),
        assigned: Vec::new(),
    };
    let mut passed = match acl.propagate(data, Reach::Children, None) {
        Ok(walked) => {
            seen.add(walked);
            true
        }
        Err(failure) => {
            tracing::warn!(%failure, "the data folder's items weren't worked out again");
            false
        }
    };
    for (child, credentials) in data_children(data) {
        let strict = credentials.then_some(&strict);
        match acl.propagate(&child, Reach::Tree, strict) {
            Ok(walked) => seen.add(walked),
            Err(failure) => {
                tracing::warn!(%failure, credentials, "a data folder's tree wasn't walked");
                passed = false;
            }
        }
    }
    if passed && let Err(error) = record_walked(data) {
        tracing::warn!(%error, "the data walk's record wasn't written; it runs again");
    }
    seen
}

/// The folders directly in `data` a data walk goes down, each with whether it holds
/// credentials; none of [`NOT_WALKED`], no link. Sorted, so walks run in one order.
fn data_children(data: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(listing) = std::fs::read_dir(data) else {
        tracing::warn!("the data folder couldn't be listed for its walk");
        return Vec::new();
    };
    let named = |name: &std::ffi::OsStr, want: &str| name.eq_ignore_ascii_case(want);
    let mut out: Vec<(PathBuf, bool)> = listing
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|t| t.is_dir() && !t.is_symlink())
        })
        .filter(|entry| !NOT_WALKED.iter().any(|n| named(&entry.file_name(), n)))
        .map(|entry| (entry.path(), named(&entry.file_name(), CREDENTIALS)))
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
