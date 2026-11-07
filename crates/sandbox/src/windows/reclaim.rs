//! The elevated clean-up (S15 `PR4i`, D-B10): what `--reclaim` walks, and the hand-back the app
//! runs after it. Pure: tested on every OS; the walk itself is `reclaim/walk.rs`, over a seam
//! whose Windows side is `acl/take.rs`.
//!
//! Some items a pool user owns block the lawyer for want of a right the lawyer lacks: a forget
//! whose hand-back stays owed (a file the agent shut the lawyer out of, under the `C:\m` layout
//! where it can't be set aside either), a launch refused as
//! [`ApplyFailure::PoolOwned`](super::writer::ApplyFailure::PoolOwned) (the lawyer may not change
//! its owner), or one refused as `ChildShut` that couldn't set the child aside. `--reclaim`
//! (elevated, with the owner taken from its verified parent, D-B2) makes the owner the owner of
//! those items, first taking off their `OWNER RIGHTS` entries (which would leave the new owner
//! without an owner's implied rights), and does nothing else; [`hand_back_targets`], run by the
//! app as the lawyer just
//! after, then cleans them as forget's hand-back does, and the app retries the forget or the
//! launch.
//!
//! [`targets`] picks the trees from the owner's ledger alone, never from a command line: each
//! matter whose hand-back is owed (its matter, state and notes trees, as forget walks them), and
//! each item a refused launch recorded (`Ledger::owe_reclaim`), for a matter `matters.json`
//! still lists as holding a pool user. A root refused as `PoolOwnedRoot` is taken alone, not its
//! tree.
//!
//! The owner writes the ledger and `matters.json`, so neither bounds what the elevated walk may
//! reveal or enter; access does (K1). A root must lie in a matter root the ledger records, both
//! re-resolved by handle to their final paths, must not be a volume's root nor hold or lie in a
//! pool user's profile ([`admissible`]), and must be a pool user's or one the owner can list;
//! otherwise it is skipped with nothing said about what is in it. The walk enters only a folder
//! a pool user owns or the owner can list, so `SeBackupPrivilege` never opens a folder neither
//! could, and its counts cover only what it reached. What it writes is the owner, and only on
//! what a pool user owns: the owner holds every pool user's password already (`secrets.bin`).
//!
//! The exe reports each item it took, by final path and file ID (`setup/taken_file.rs`, written
//! as the owner beside its result; it never writes the ledger). [`hand_back_after`], as the
//! lawyer, records them in the ledger as [`Stuck::Taken`] and strips, on those items alone, the
//! denies an agent left of the lawyer, its token's groups or an owner's stand-in, so none can
//! later refuse the lawyer's label write; the record goes once a hand-back refuses nothing. The
//! lawyer's own files and the matter roots keep their denies: they weren't taken.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};

use super::acl_plan::{Inherit, in_folder};
use super::apply::OWN_KINDS;
use super::ledger::{Ledger, ReclaimOwed, Stuck, clear_taken, owe_taken};
use super::matters::Rows;
use super::sids::Sid;
use super::writer::{AclWriter, ApplyFailure, DenyStrip, FileId};

pub mod walk;

pub use walk::{OwnerTree, Reclaimed, Scope, take_ownership};

/// Why a tree is walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Why {
    /// The matter's forget owes a hand-back.
    Owed,
    /// A launch of the matter refused at this item.
    Stuck(Stuck),
}

/// One tree the clean-up walks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReclaimRoot {
    /// The matter it is walked for.
    pub matter: MatterId,
    /// Its root, as the ledger names it.
    pub root: PathBuf,
    /// Whether everything below the root is walked too, or the root alone.
    pub whole: bool,
    /// Why it is walked.
    pub why: Why,
    /// The matter's own pool user, whose allows the hand-back after keeps: the one it holds
    /// (`matters.json`) for a refused launch, none for a matter being forgotten.
    pub own: Vec<Sid>,
    /// Our own allows the ledger lists at or below the root (path and SID), which the
    /// hand-back after keeps: a credential link's, another matter's root grants.
    pub keep: Vec<(PathBuf, Sid)>,
}

/// The matter, state and notes folders the ledger records, for any matter: the only places a
/// clean-up may start in ([`Scope::bounds`]).
pub fn matter_roots(ledger: &Ledger) -> Vec<PathBuf> {
    let roots = ledger
        .entries()
        .filter(|e| OWN_KINDS.contains(&e.kind) && e.rights.is_allow())
        .map(|e| e.path.clone());
    roots.collect::<BTreeSet<_>>().into_iter().collect()
}

/// The trees to walk, from `ledger` and `matters` (`matters.json`'s rows) alone, as the module
/// says: outermost first, none inside another tree that is walked whole, and none outside every
/// matter root the ledger records ([`matter_roots`]; the walk checks that again on the final
/// paths).
pub fn targets(ledger: &Ledger, matters: &Rows) -> Vec<ReclaimRoot> {
    let owed: BTreeSet<MatterId> = ledger.handbacks_owed().iter().map(|o| o.matter).collect();
    let bounds = matter_roots(ledger);
    let mut found: Vec<(PathBuf, MatterId, bool, Why, Vec<Sid>)> = Vec::new();
    for entry in ledger.entries() {
        let own = OWN_KINDS.contains(&entry.kind)
            && entry.rights.is_allow()
            && entry.inherit == Inherit::Tree;
        if own && owed.contains(&entry.matter) {
            found.push((
                entry.path.clone(),
                entry.matter,
                true,
                Why::Owed,
                Vec::new(),
            ));
        }
    }
    for stuck in ledger.reclaims_owed() {
        let held = matters.get(&stuck.matter).and_then(|row| row.holds());
        if held.is_none() && !owed.contains(&stuck.matter) {
            tracing::debug!(matter = %stuck.matter, "a clean-up for a matter with no pool user");
            continue;
        }
        if !bounds.iter().any(|b| in_folder(&stuck.path, b)) {
            tracing::warn!(matter = %stuck.matter, "a clean-up outside every matter is refused");
            continue;
        }
        let whole = !matches!(stuck.why, Stuck::PoolOwnedRoot | Stuck::Taken);
        let own: Vec<Sid> = held.into_iter().cloned().collect();
        found.push((stuck.path, stuck.matter, whole, Why::Stuck(stuck.why), own));
    }
    found.sort_by(|a, b| {
        let depth = |p: &Path| p.components().count();
        (depth(&a.0), &a.0, !a.2, a.3).cmp(&(depth(&b.0), &b.0, !b.2, b.3))
    });
    let mut kept: Vec<ReclaimRoot> = Vec::new();
    for (root, matter, whole, why, own) in found {
        let covered = kept
            .iter()
            .any(|k| in_folder(&root, &k.root) && (k.whole || in_folder(&k.root, &root)));
        if covered {
            continue;
        }
        let keep = ledger
            .entries()
            .filter(|e| e.rights.is_allow() && in_folder(&e.path, &root))
            .map(|e| (e.path.clone(), e.sid.clone()))
            .collect();
        kept.push(ReclaimRoot {
            matter,
            root,
            whole,
            why,
            own,
            keep,
        });
    }
    kept
}

/// Whether `root` (a final path, as its handle gives it) may be walked: absolute, not a
/// volume's root, inside one of `bounds` (the recorded matter roots, also final paths), and
/// neither inside nor holding any of `profiles` (the pool users' profile folders, from
/// `ProfileList`, which only an administrator can write). Compared as `in_folder` compares:
/// verbatim and without ASCII case; short (8.3) names are resolved before, by handle.
pub fn admissible(root: &Path, bounds: &[PathBuf], profiles: &[PathBuf]) -> bool {
    let volume = root.parent().is_none();
    let bounded = bounds.iter().any(|b| in_folder(root, b));
    let profile = profiles
        .iter()
        .any(|p| in_folder(root, p) || in_folder(p, root));
    root.is_absolute() && !volume && bounded && !profile
}

/// What [`hand_back_targets`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HandedBack {
    /// Roots handed back, alone or with their trees, with nothing refused.
    pub done: usize,
    /// Each refusal: the failure, and the item it named when it named one (for the lawyer;
    /// never logged).
    pub refused: Vec<(ApplyFailure, Option<PathBuf>)>,
}

/// The lawyer's hand-back after `--reclaim`, for each target, with `pool` (the machine
/// record's pool users) less the target's own pool user: main's hand-back of the root
/// ([`AclWriter::hand_back`]), [`AclWriter::strip_taken`] on what the lawyer owns now, which
/// loses only those pool users' explicit allows, our own (`keep`) kept, and on the items
/// `denies` lists as taken the denies an agent left of the lawyer; then main's hand-back below
/// a root walked whole ([`AclWriter::hand_back_tree`]) of what a pool user still owns. A
/// [`Stuck::Taken`] root, the lawyer's already, gets the strip alone. A refusal is listed and
/// the next target tried.
pub fn hand_back_targets(
    acl: &mut dyn AclWriter,
    targets: &[ReclaimRoot],
    pool: &[Sid],
    denies: &DenyStrip<'_>,
) -> HandedBack {
    let mut out = HandedBack::default();
    for target in targets {
        let others: Vec<Sid> = pool
            .iter()
            .filter(|p| !target.own.contains(p))
            .cloned()
            .collect();
        let root = &target.root;
        // The strip goes before main's hand-back below the root: a folder `--reclaim` took that
        // only its agent could list is the lawyer's to open only once it is stripped (and,
        // left bare, unprotected).
        let taken = target.why == Why::Stuck(Stuck::Taken);
        let done = if taken {
            Ok(())
        } else {
            acl.hand_back(root, &others).map(drop)
        };
        let done = done
            .and_then(|()| {
                acl.strip_taken(root, &others, &target.keep, target.whole, denies)
                    .map(drop)
            })
            .and_then(|()| {
                if target.whole && !taken {
                    acl.hand_back_tree(root, &others).map(drop)
                } else {
                    Ok(())
                }
            });
        match done {
            Ok(()) => out.done += 1,
            Err(failure) => {
                tracing::warn!(%failure, "an item the clean-up took wasn't handed back");
                out.refused.push((failure, acl.refused_at()));
            }
        }
    }
    out
}

/// One item `--reclaim` took, as its result reports it (`setup/taken_file.rs`): the matter
/// whose target it was walked for, its final path as its handle gave it, and its file ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TakenItem {
    /// The matter of the target it was found in.
    pub matter: MatterId,
    /// Its final path.
    pub path: PathBuf,
    /// Its file ID.
    pub file: FileId,
}

/// The lawyer's whole hand-back after `--reclaim`: the items it reported taken (`taken`)
/// recorded in the ledger as [`Stuck::Taken`] (so one a refused hand-back leaves is tried again
/// by the next), the targets read again from the ledger and `matters`, each handed back
/// ([`hand_back_targets`]) with denies stripped on every taken item the ledger lists, keeping
/// the denies of `kept` (the record's pool users and pool group); then, when nothing was
/// refused, those taken items cleared. A record that can't be written is logged, and the items
/// reported are still stripped this time.
pub fn hand_back_after(
    acl: &mut dyn AclWriter,
    ledger: &mut Ledger,
    matters: &Rows,
    taken: &[TakenItem],
    pool: &[Sid],
    kept: &[Sid],
) -> io::Result<HandedBack> {
    let reported: Vec<(MatterId, PathBuf, FileId)> = taken
        .iter()
        .map(|t| (t.matter, t.path.clone(), t.file))
        .collect();
    if !reported.is_empty()
        && let Err(error) = owe_taken(ledger, &reported)
    {
        tracing::warn!(error = %error.kind(), "the items taken weren't recorded");
    }
    let owed: Vec<ReclaimOwed> = ledger
        .reclaims_owed()
        .into_iter()
        .filter(|o| o.why == Stuck::Taken && o.file.is_some())
        .collect();
    let mut listed: Vec<(PathBuf, FileId)> = owed
        .iter()
        .filter_map(|o| o.file.map(|file| (o.path.clone(), file)))
        .collect();
    for (_, path, file) in reported {
        if !listed.iter().any(|(p, f)| *p == path && *f == file) {
            listed.push((path, file));
        }
    }
    let found = targets(ledger, matters);
    let denies = DenyStrip {
        taken: &listed,
        kept,
    };
    let handed = hand_back_targets(acl, &found, pool, &denies);
    if handed.refused.is_empty() {
        clear_taken(ledger, &owed)?;
    }
    Ok(handed)
}

#[cfg(test)]
mod taken_tests;
#[cfg(test)]
mod tests;
