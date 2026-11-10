//! The fence's deny on a matter folder, split in two (R18): the folder alone, and everything
//! below it (`fence.rs`). A ledger written before lists the one entry for the whole tree; it
//! doesn't list the split fence as applied, so the next launch (or the pool's growth) plans
//! it. Once that launch has written and walked the two entries, [`retire_whole`] takes the old
//! one back: the folder and every child stay denied throughout, by the old entry until the two
//! are written and by the two after. The old entry's inherited copies are the same deny as the
//! new one's, and the walk after the take-back leaves only the new ones.

use std::path::PathBuf;

use clauseo_protocol::MatterId;

use super::Surroundings;
use super::back::{finish, take_back};
use crate::windows::acl_plan::{AdmittedPlan, Inherit, Rights, Sid};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{Entry, Ledger, Take};
use crate::windows::writer::AclWriter;

/// After `plan` applied: takes back each whole-tree fence entry on a matter folder whose split
/// pair `plan` holds (same matter, folder and pool user). A failure is logged and leaves the
/// old entry `removing` for [`reconcile`](super::reconcile): a deny left over closes more,
/// never less (a read of the whole folder can't list it until then).
pub(super) fn retire_whole(
    ledger: &mut Ledger,
    plan: &AdmittedPlan,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) {
    let split: Vec<(MatterId, PathBuf, Sid)> = plan
        .grants()
        .iter()
        .zip(plan.owners())
        .filter(|(g, _)| fence_on_folder(g.kind(), g.rights()) && g.inherit() == Inherit::Below)
        .map(|(g, owner)| (*owner, g.path().to_owned(), g.sid().clone()))
        .collect();
    if split.is_empty() {
        return;
    }
    let pick = |e: &Entry| {
        let whole = fence_on_folder(e.kind, e.rights) && e.inherit == Inherit::Tree;
        let paired = split
            .iter()
            .any(|(m, p, s)| *m == e.matter && *p == e.path && *s == e.sid);
        (whole && paired).then_some(Take::Remove)
    };
    let taken = match take_back(ledger, None, acl, pick) {
        Ok(taken) => taken,
        Err(error) => {
            let error = error.kind();
            tracing::warn!(%error, "the whole-tree fence entries weren't taken back");
            return;
        }
    };
    if taken.removed.is_empty() && taken.kept == 0 {
        return;
    }
    let count = taken.removed.len();
    match finish(ledger, acl, around, &taken, None, &[]) {
        Ok(_) => tracing::info!(count, "tier B split the fence on matter folders"),
        Err(refused) => {
            let failure = refused.failure;
            tracing::warn!(%failure, "a whole-tree fence entry stays removing");
        }
    }
}

/// Whether an entry of `kind` and `rights` is a fence's deny on a matter folder.
fn fence_on_folder(kind: PathKind, rights: Rights) -> bool {
    kind == PathKind::Matter && rights == Rights::DenyReadWrite
}
