//! Step 3 of [`apply`](super::apply) (S15 `PR4h`): no pool user owns a root the plan denies on.
//! An object's owner may always change its permissions, so a pool user owning one could take
//! off the deny we write there. Each such root goes to the desktop user first, as a protected
//! item does ([`AclWriter::hand_back`], S15 `PR4a2`); only a root whose hand-back fails refuses.

use std::collections::BTreeSet;
use std::path::Path;

use super::strict_for;
use crate::Error;
use crate::windows::acl_plan::{AdmittedPlan, Sid};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{Entry, EntryState, Ledger};
use crate::windows::writer::{AclWriter, ApplyFailure, Owned};

/// The kinds of root handed back when a pool user owns it: rare (a folder an agent made, moved
/// by the lawyer and registered, or made next to the matter).
pub(super) const ROOT_KINDS: [PathKind; 6] = [
    PathKind::Matter,
    PathKind::State,
    PathKind::Notes,
    PathKind::OtherMatter,
    PathKind::Sibling,
    PathKind::ParentFiles,
];

/// Before anything is recorded, each root of `plan` ([`ROOT_KINDS`]) that a pool SID owns (one
/// `plan` grants or denies: its pool user, the others, the group; or one of `extra`) is handed
/// to the desktop user ([`AclWriter::hand_back`]: the root alone, cleaned against its folder).
/// Nothing is recorded: the lawyer owning it is where forget leaves an agent's items too.
///
/// The cleaning strips the pool's explicit allows and our label from the root, the matter's
/// own `Modify` included, which `ledger` may list as applied; a fence-only plan (set-up's
/// extend) doesn't plan those grants. So every entry `ledger` lists on a root handed back is
/// written again here, at once ([`AclWriter::add`] leaves one still there as it is).
///
/// A hand-back refused for want of a right ([`ApplyFailure::ChildShut`]: the handle got no
/// `WRITE_OWNER` or `WRITE_DAC`), or that read back still another's or with a pool user's allow
/// on it ([`ApplyFailure::PoolOwned`]), refuses as `PoolOwned` ([`Owned::Root`]), naming the
/// root; any other failure refuses as itself. A root another user owns, or a missing one, is
/// left to the writes that follow.
pub(super) fn hand_back_roots(
    plan: &AdmittedPlan,
    ledger: &Ledger,
    acl: &mut dyn AclWriter,
    extra: &[Sid],
) -> Result<(), Error> {
    let mut pool = strict_for(plan).pool;
    pool.extend(extra.iter().cloned());
    pool.sort();
    pool.dedup();
    let mut seen: BTreeSet<&Path> = BTreeSet::new();
    for grant in plan.grants() {
        let (kind, path) = (grant.kind(), grant.path());
        if !ROOT_KINDS.contains(&kind) || !seen.insert(path) {
            continue;
        }
        let refuse = |failure| Error::AclApply {
            path_kind: kind,
            failure,
            at: Some(path.to_owned()),
        };
        let failure = match acl.hand_back(path, &pool) {
            Ok(false) => continue,
            Ok(true) => {
                restore_entries(ledger, acl, path).map_err(refuse)?;
                continue;
            }
            Err(ApplyFailure::ChildShut | ApplyFailure::PoolOwned { .. }) => {
                ApplyFailure::PoolOwned { owned: Owned::Root }
            }
            Err(failure) => failure,
        };
        tracing::warn!(%kind, %failure, "a root a sandbox user owns wasn't handed back");
        return Err(refuse(failure));
    }
    Ok(())
}

/// Writes again each entry `ledger` lists, applied or pending, on `path`, a root just handed
/// back (the credential link's aside: it is a file in the state folder, never a root).
fn restore_entries(
    ledger: &Ledger,
    acl: &mut dyn AclWriter,
    path: &Path,
) -> Result<(), ApplyFailure> {
    let listed = ledger.entries().filter(|e| {
        e.path == path
            && e.kind != PathKind::CredentialLink
            && matches!(e.state, EntryState::Applied | EntryState::Pending)
    });
    let grants: Vec<_> = listed.map(Entry::grant).collect();
    for grant in &grants {
        acl.add(grant)?;
    }
    Ok(())
}
