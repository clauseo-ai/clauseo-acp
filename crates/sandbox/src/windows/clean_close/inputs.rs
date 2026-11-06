//! What the own-tree walks depend on besides the trees: the own roots, the matter's grants on
//! them, and the strict sets. The record keeps these as the session-end repair used them (from
//! the ledger), and a launch compares them, exactly, with what its walk would use (from the
//! plan). Kept whole rather than hashed: an exact comparison, a few hundred bytes. The read-only
//! denies aren't among them: each is checked at every launch, with its own walk when it is
//! written (`apply_checked`'s docs).

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::windows::acl_plan::{AdmittedPlan, Grant, Rights, Sid};
use crate::windows::admit::PathKind;
use crate::windows::apply::{OWN_KINDS, strict_for};
use crate::windows::writer::Strict;

/// The inputs of a matter's own-tree walks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WalkInputs {
    /// The matter, state and notes folders, sorted.
    pub(crate) roots: Vec<(PathBuf, PathKind)>,
    /// The matter's grants on those folders, sorted.
    grants: Vec<Grant>,
    /// [`Strict::watched`].
    watched: Vec<Sid>,
    /// [`Strict::pool`].
    pool: Vec<Sid>,
}

impl WalkInputs {
    /// What a launch's own-tree walks of `plan` use: its `Modify` grants of the own kinds, its
    /// grants on those folders, and [`strict_for`] the plan.
    pub(crate) fn of_plan(plan: &AdmittedPlan) -> WalkInputs {
        let mine: Vec<&Grant> = plan
            .grants()
            .iter()
            .zip(plan.owners())
            .filter(|(_, owner)| **owner == plan.matter())
            .map(|(grant, _)| grant)
            .collect();
        WalkInputs::of(&mine, &strict_for(plan))
    }

    /// What `repair_after_session` used for `matter`: its ledger entries, as last read.
    #[cfg(any(windows, test))]
    pub(crate) fn of_ledger(
        ledger: &crate::windows::ledger::Ledger,
        matter: clauseo_protocol::MatterId,
    ) -> WalkInputs {
        use crate::windows::ledger::Entry;
        let entries: Vec<Entry> = ledger.matter_entries(matter);
        let grants: Vec<Grant> = entries.iter().map(Entry::grant).collect();
        let mine: Vec<&Grant> = grants.iter().collect();
        WalkInputs::of(&mine, &crate::windows::apply::strict_of(&entries))
    }

    fn of(mine: &[&Grant], strict: &Strict) -> WalkInputs {
        let roots: BTreeSet<(PathBuf, PathKind)> = mine
            .iter()
            .filter(|g| g.rights() == Rights::Modify && OWN_KINDS.contains(&g.kind()))
            .map(|g| (g.path().to_owned(), g.kind()))
            .collect();
        let mut grants: Vec<Grant> = mine
            .iter()
            .filter(|g| roots.iter().any(|(root, _)| root == g.path()))
            .map(|g| (*g).clone())
            .collect();
        grants.sort_by(|a, b| {
            let key = |g: &Grant| {
                (
                    g.path().to_owned(),
                    g.sid().clone(),
                    g.rights(),
                    g.inherit(),
                )
            };
            key(a).cmp(&key(b)).then(a.kind().cmp(&b.kind()))
        });
        grants.dedup();
        WalkInputs {
            roots: roots.into_iter().collect(),
            grants,
            watched: strict.watched.clone(),
            pool: strict.pool.clone(),
        }
    }
}
