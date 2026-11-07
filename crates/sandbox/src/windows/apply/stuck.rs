//! The clean-ups a refused launch leaves for the elevated `--reclaim` (S15 `PR4i`): a refusal
//! as [`ApplyFailure::PoolOwned`] or [`ApplyFailure::ChildShut`] records its item in the ledger
//! (`Ledger::owe_reclaim`), as the lawyer, under the ledger's lock; `--reclaim` walks only what
//! is recorded there. A launch of the matter that succeeds clears the matter's list.

use crate::Error;
use crate::windows::acl_plan::AdmittedPlan;
use crate::windows::ledger::{Ledger, Stuck};
use crate::windows::writer::{ApplyFailure, Owned};

/// Records the clean-up `error` asks for, when it is a refusal `--reclaim` can lift that names
/// its item. One naming none records nothing: walking the launch's whole trees would hand back
/// our own entries' items too. A record that fails is logged, never in place of the refusal.
pub(super) fn owe(ledger: &mut Ledger, plan: &AdmittedPlan, error: &Error) {
    let Error::AclApply { failure, at, .. } = error else {
        return;
    };
    let why = match failure {
        ApplyFailure::PoolOwned { owned: Owned::Root } => Stuck::PoolOwnedRoot,
        ApplyFailure::PoolOwned {
            owned: Owned::Child,
        } => Stuck::PoolOwnedChild,
        ApplyFailure::ChildShut => Stuck::ChildShut,
        ApplyFailure::ProtectedChild
        | ApplyFailure::BroadAllowChild
        | ApplyFailure::Changed
        | ApplyFailure::LinkedFile
        | ApplyFailure::VolumeRoot
        | ApplyFailure::NoDacl
        | ApplyFailure::Io(_)
        | ApplyFailure::Ledger(_)
        | ApplyFailure::Link
        | ApplyFailure::CantLabel => return,
    };
    let Some(at) = at else {
        tracing::warn!(?why, "a refusal named no item; no clean-up recorded");
        return;
    };
    if let Err(error) = ledger.owe_reclaim(plan.matter(), at.clone(), why) {
        let error = error.kind();
        tracing::warn!(%error, ?why, "a clean-up for the elevated set-up wasn't recorded");
    }
}

/// Clears the matter's clean-ups once its launch has succeeded; a failure is logged.
pub(super) fn settled(ledger: &mut Ledger, plan: &AdmittedPlan) {
    if let Err(error) = ledger.clear_reclaims(plan.matter()) {
        let error = error.kind();
        tracing::warn!(%error, "a matter's clean-ups owed weren't cleared");
    }
}
