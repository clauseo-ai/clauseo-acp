//! After a launch succeeds (S15 PR3, the review's H3 and M1): a folder that is now a registered
//! matter loses the sibling deny on it, for every matter whose entry names it, and its tree is
//! walked again. A deny on a folder that holds the matter is first replaced, for the matter that
//! wrote it, by the gate's walk-down set below that folder around the new matter's folders:
//! a deny on each folder beside the path at every level, and a files-only deny at each level.
//! A refused launch never gets here, so it leaves every deny; a lift or replacement that fails
//! leaves its deny too (the new matter's own entries still reach its agent, nearer than it), and
//! the next successful launch tries again.

#[cfg(any(windows, test, feature = "test-support"))]
use std::collections::BTreeSet;
#[cfg(any(windows, test, feature = "test-support"))]
use std::path::Path;
use std::path::PathBuf;

use super::Surroundings;
use super::back::take_back;
#[cfg(any(windows, test, feature = "test-support"))]
use super::run_plan;
use crate::windows::acl_plan::{AdmittedPlan, Rights, in_folder};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{Entry, Ledger, Take};
use crate::windows::writer::AclWriter;
use crate::windows::writer::ApplyFailure;

/// Lifts the sibling denies on `roots` (the plan's matter folders) or on a folder holding one;
/// see the module's docs. Returns how many entries were dropped.
pub(super) fn lift(
    ledger: &mut Ledger,
    plan: &AdmittedPlan,
    roots: &[PathBuf],
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> usize {
    let denies: Vec<Entry> = ledger
        .entries()
        .filter(|e| {
            e.kind == PathKind::Sibling
                && e.rights == Rights::DenyRead
                && roots.iter().any(|root| in_folder(root, &e.path))
        })
        .cloned()
        .collect();
    let mut lifting: Vec<Entry> = Vec::new();
    for deny in denies {
        let exact = roots.contains(&deny.path);
        if exact || replace(ledger, plan, &deny, acl, around) {
            lifting.push(deny);
        }
    }
    if lifting.is_empty() {
        return 0;
    }
    let pick = |entry: &Entry| {
        lifting
            .iter()
            .any(|l| {
                // N12: the matter too, so a matter whose replacement failed keeps its deny.
                l.matter == entry.matter
                    && l.path == entry.path
                    && l.sid == entry.sid
                    && l.rights == entry.rights
                    && l.inherit == entry.inherit
            })
            .then_some(Take::Remove)
    };
    let taken = match take_back(ledger, None, acl, pick) {
        Ok(taken) => taken,
        Err(error) => {
            let error = error.kind();
            tracing::warn!(%error, "a sibling deny wasn't lifted; it stays");
            return 0;
        }
    };
    if let Err(refused) = super::back::finish(ledger, acl, around, &taken, None, &[]) {
        let failure: ApplyFailure = refused.failure;
        tracing::warn!(%failure, "a lifted folder's walk failed; its entries stay removing");
    }
    taken.removed.len() + taken.shared
}

/// Replaces `deny`, a sibling deny on a folder holding the new matter, by the walk-down set
/// below that folder for the matter that wrote it: planned with the gate's own walk around the
/// new matter's folders inside it, and applied as that matter's entries. Whether it worked.
#[cfg(any(windows, test, feature = "test-support"))]
fn replace(
    ledger: &mut Ledger,
    plan: &AdmittedPlan,
    deny: &Entry,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> bool {
    let protected: Vec<PathBuf> = plan
        .grants()
        .iter()
        .zip(plan.owners())
        .filter(|(g, owner)| **owner == plan.matter() && g.rights().is_allow())
        .map(|(g, _)| g.path().to_owned())
        .filter(|path| in_folder(path, &deny.path) && path.as_path() != deny.path.as_path())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let set = {
        let check = |p: &Path| around.check(p);
        crate::windows::admit::walk_down(&deny.path, &protected, check, &*around)
    };
    let set = match set {
        Ok(set) => set,
        Err(error) => {
            tracing::warn!(%error, "the walk-down set wasn't planned; the deny stays");
            return false;
        }
    };
    let mut sub =
        AdmittedPlan::walk_down_denies(deny.matter, &deny.sid, &set.siblings, &set.levels);
    match run_plan(ledger, &mut sub, acl, around) {
        Ok(_) => true,
        Err(error) => {
            tracing::warn!(%error, "the walk-down set wasn't applied; the deny stays");
            false
        }
    }
}

/// Without the gate's walk (a build for neither Windows nor tests), nothing is replaced: the
/// deny stays.
#[cfg(not(any(windows, test, feature = "test-support")))]
fn replace(
    _ledger: &mut Ledger,
    _plan: &AdmittedPlan,
    _deny: &Entry,
    _acl: &mut dyn AclWriter,
    _around: &mut dyn Surroundings,
) -> bool {
    false
}
