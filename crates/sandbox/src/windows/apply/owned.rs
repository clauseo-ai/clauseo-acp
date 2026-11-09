//! Step 3b of [`apply`](super::apply): each folder Clauseo owns for the launch
//! ([`AdmittedPlan::owned_roots`]: the data folder, its `agent-state` and `run` folders, the
//! matter's, the state, notes, run and temp folders) gets its protected DACL, after the roots'
//! hand-back and before anything is recorded. Nothing is recorded for it, and a rollback leaves
//! it: the folder is Clauseo's, and a protected DACL with the lawyer's and `SYSTEM`'s full access
//! is where it belongs whatever the launch does. One whose DACL this changed has its tree walked
//! with the launch's other walks, so what is in it already loses the entries it inherited from
//! above (`C:\`'s `Users` entries); the data folder and its shared folders are left to each
//! matter's and session's own launch.

use super::strict_for;
use super::walk::Walk;
use crate::Error;
use crate::windows::acl_plan::{AdmittedPlan, OwnedKind, Sid};
use crate::windows::writer::{AclWriter, Added, Reach};

/// Protects each of `plan`'s owned roots through `acl`, keeping the explicit denies for the pool
/// SIDs `plan` names and `pool`; the walks the roots whose DACL changed need (strict for the
/// state and notes folders, as the launch's own trees are). The first refusal refuses the launch,
/// naming the root.
pub(super) fn protect_owned(
    plan: &AdmittedPlan,
    acl: &mut dyn AclWriter,
    pool: &[Sid],
) -> Result<Vec<Walk>, Error> {
    let mut sids = strict_for(plan).pool;
    sids.extend(pool.iter().cloned());
    sids.sort();
    sids.dedup();
    let mut walks = Vec::new();
    for root in plan.owned_roots() {
        let added = acl
            .protect(root, &sids)
            .map_err(|failure| Error::AclApply {
                path_kind: root.path_kind(),
                failure,
                at: Some(root.path().to_owned()),
            })?;
        if added == Added::Written && root.walked() {
            walks.push(Walk {
                root: root.path().to_owned(),
                reach: Reach::Tree,
                strict: matches!(root.kind(), OwnedKind::State | OwnedKind::Notes),
                kind: root.path_kind(),
            });
        }
    }
    if !walks.is_empty() {
        tracing::info!(
            roots = walks.len(),
            "owned folders protected; their trees are walked"
        );
    }
    Ok(walks)
}
