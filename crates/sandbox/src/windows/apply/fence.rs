//! The fences of the registered matters as a whole, when the pool grows: [`fence_registered`]
//! for one plan of them all, and [`fence_each`] for set-up's step 8, which leaves out a matter
//! a pool user owns part of that couldn't be handed back (S15 `PR4h`).

use clauseo_protocol::MatterId;

use super::{Applied, Surroundings, apply_with_pool};
use crate::Error;
use crate::windows::acl_plan::{AdmittedPlan, Sid, in_folder};
use crate::windows::clean_close::{FullReason, LaunchCheck};
use crate::windows::fence::RegisteredMatter;
use crate::windows::ledger::Ledger;
use crate::windows::writer::{AclWriter, ApplyFailure};

/// Fences every registered matter against the whole `pool`, as when the pool grows: each
/// matter's folders resolved and checked as the gate checks a neighbour's (`around`'s folder
/// check), then the fence planned and applied. Entries listed already are kept, so only the new
/// pool users' denies are written. PR4's set-up calls this after adding users.
pub fn fence_registered(
    ledger: &mut Ledger,
    registered: &[RegisteredMatter],
    pool: &[Sid],
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> Result<Applied, Error> {
    let fences = crate::windows::admit::fences_for(registered, |p| around.check(p))?;
    let Some(mut plan) = AdmittedPlan::from_fences(&fences, pool) else {
        return Ok(Applied::default());
    };
    let full = LaunchCheck::full(FullReason::NoRecord);
    apply_with_pool(ledger, &mut plan, acl, around, &full, pool)
}

/// What [`fence_each`] did: the fences it applied, and each matter it skipped with why.
#[derive(Debug, Default)]
pub struct FencedEach {
    /// The fences applied, all matters' together.
    pub applied: Applied,
    /// Each matter skipped, and its refusal.
    pub skipped: Vec<(MatterId, Error)>,
}

/// [`fence_registered`] for set-up's step 8 (S15 `PR4h`): a matter whose fence refuses because a
/// pool user owns its root and the hand-back to the desktop user failed
/// ([`ApplyFailure::PoolOwned`], the matter found by the path refused) is left out and the rest
/// applied again, until one plan of the others applies. Each try is one plan, rolled back whole
/// when it fails, so any other failure (a folder check, the ledger, a write) is the `Err` with
/// nothing of this fence left.
pub fn fence_each(
    ledger: &mut Ledger,
    registered: &[RegisteredMatter],
    pool: &[Sid],
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> Result<FencedEach, Error> {
    let mut fences = crate::windows::admit::fences_for(registered, |p| around.check(p))?;
    let full = LaunchCheck::full(FullReason::NoRecord);
    let mut skipped = Vec::new();
    loop {
        let Some(mut plan) = AdmittedPlan::from_fences(&fences, pool) else {
            let applied = Applied::default();
            return Ok(FencedEach { applied, skipped });
        };
        let error = match apply_with_pool(ledger, &mut plan, acl, around, &full, pool) {
            Ok(applied) => return Ok(FencedEach { applied, skipped }),
            Err(error) => error,
        };
        let Error::AclApply {
            failure: ApplyFailure::PoolOwned { .. },
            at: Some(at),
            ..
        } = &error
        else {
            return Err(error);
        };
        // The matter whose folder holds the refused path most closely.
        let holding = fences
            .iter()
            .enumerate()
            .filter_map(|(i, fence)| {
                let inside = fence.folders.iter().filter(|(f, _)| in_folder(at, f));
                inside
                    .map(|(f, _)| f.components().count())
                    .max()
                    .map(|d| (i, d))
            })
            .max_by_key(|&(_, depth)| depth);
        let Some((i, _)) = holding else {
            return Err(error);
        };
        skipped.push((fences.remove(i).id, error));
    }
}
