//! The reads of other matters' folders (R18, decision 0049) live only while a session does:
//! [`apply`](super::apply) writes them with the rest of the launch, and they are taken back at
//! the run's end ([`end_reads`]), when the lawyer forgets one mid-session ([`narrow_reads`]), and
//! at the matter's next launch for any a crash left ([`apply_checked`](super::apply_checked)
//! takes back each one its plan doesn't list before it records anything). [`forget_matter`]
//! takes them back with the rest of the matter's entries. A pool user serves another matter only
//! once none of its allows is listed (the reuse check), so a read left by a crash never follows
//! the pool user to another matter.
//!
//! Each take-back is the ledger's own ([`Ledger::take_back`]): the entries marked `removing`
//! on disk first, the allows removed, each tree walked again so the inherited copies go too,
//! and only then the entries dropped. A removal or walk that fails leaves its entries
//! `removing`, for the next call, [`reconcile`](super::reconcile) or the next launch.
//!
//! Each read's entry records the item it is written on, by its file ID (`RecordedFile`), before
//! the allow is written, and the allow is written only on the item with that ID
//! ([`AclWriter::add_recorded`]). The take-back removes it from that item, opened by its ID
//! wherever it is now, and walks its tree there (`back.rs`): a granted folder the other matter's
//! agent or the lawyer moved during the run loses the allow where it went; an item deleted took
//! its allow with it; a new item at the same path is left alone. A launch takes back, as stale,
//! each read whose item isn't at its path any more, or that recorded none (an older ledger's),
//! and writes it again on the item there now.
//!
//! Before any allow is written, the granted tree is held to the strict walk's rule
//! ([`check_trees`]): an item with inheritance off, which the fence's deny doesn't reach, or with
//! an allow beating it, refuses the launch.
//!
//! Each call takes back only the entries naming the session's own SIDs (`sids`: its pool user,
//! and the matter's SID when that is a subject, off by default), so a second session of the
//! matter, with a pool user of its own, keeps its reads. The matter's SID is the matter's, not
//! one session's: with it on, any session's end takes its reads back for both, the safe way.
//!
//! [`forget_matter`]: super::forget_matter

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::back::{finish, take_back};
use super::lifts;
use super::walk::{Refused, clearing};
use super::{Surroundings, refused};
use crate::windows::acl_plan::{AdmittedPlan, Grant, Inherit, Rights, Sid, in_folder};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{Entry, EntryId, Ledger, RecordedFile, Take};
use crate::windows::writer::{AclWriter, Added, ApplyFailure, Reach, Strict, Walked};
use crate::{Error, OtherMatterRead, Policy};

/// What [`narrow_reads`] or [`end_reads`] took back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Narrowed {
    /// Each path whose allow for the matter's pool user (and its matter SID, when that is a
    /// subject) was removed, its tree walked again and its ledger entry dropped, in path order.
    /// Empty when there was nothing to take back.
    pub removed: Vec<PathBuf>,
    /// Each other matter's folder whose fence deny, lifted for a read of the whole folder, is
    /// back (`lifts.rs`), in the order put back.
    pub restored: Vec<PathBuf>,
}

/// When the lawyer forgets a read of another matter mid-session: takes back at once each of
/// `matter`'s reads of other matters naming one of `sids` (the session's pool user, and the
/// matter's SID when the launch named it) that `policy` no longer holds, and never adds one
/// (the host relaunches at the next turn boundary for that). An entry is held when it is on or
/// inside one of `policy`'s [`Policy::other_matter_reads`] (as a grant's own allows all are),
/// so a grant narrowed to a folder inside it keeps the allows inside that folder and loses the
/// rest. A read the new grant holds that came by inheritance from an entry above it (a file
/// whose folder's allow for its files is dropped, when the grant narrows to that file) goes
/// with that entry until the relaunch plans the new grant's own allows: narrowing fails closed,
/// never open. Idempotent: a second call with the same policy takes nothing. A handle the agent
/// opened before stays open until the host's relaunch.
///
/// An error leaves each entry it couldn't take back `removing`, for the next call.
pub fn narrow_reads(
    ledger: &mut Ledger,
    matter: MatterId,
    sids: &[Sid],
    policy: &Policy,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> Result<Narrowed, Error> {
    let reads = policy.other_matter_reads();
    let held = |entry: &Entry| {
        reads
            .iter()
            .any(|read| in_folder(&entry.path, read.path()) && !read.refuses(&entry.path))
    };
    // A lifted deny stays off while the policy still reads that whole folder.
    let whole = |e: &Entry| {
        let same = |read: &&OtherMatterRead| same_path(read.path(), &e.path);
        e.lifted == Some(matter) && sids.contains(&e.sid) && !reads.iter().any(|r| same(&r))
    };
    let restored = lifts::restore(ledger, acl, whole);
    finished(
        restored,
        take_reads(ledger, matter, sids, held, acl, around),
    )
}

/// At the session's end: takes back every one of `matter`'s reads of other matters naming one
/// of `sids`, as [`narrow_reads`] with a policy holding none. PR6's launcher calls it
/// after `repair_after_session`, whatever that returned.
pub fn end_reads(
    ledger: &mut Ledger,
    matter: MatterId,
    sids: &[Sid],
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> Result<Narrowed, Error> {
    let mine = |e: &Entry| e.lifted == Some(matter) && sids.contains(&e.sid);
    let restored = lifts::restore(ledger, acl, mine);
    finished(
        restored,
        take_reads(ledger, matter, sids, |_| false, acl, around),
    )
}

/// The take-back's result with the restored folders, or the first error: a deny not put back
/// is an error even when every allow went (its mark stays, for the next call).
fn finished(
    restored: Result<Vec<PathBuf>, Error>,
    taken: Result<Narrowed, Error>,
) -> Result<Narrowed, Error> {
    let restored = restored?;
    Ok(Narrowed { restored, ..taken? })
}

/// At a launch, before anything is recorded: takes back each of the plan's matter's reads of
/// other matters naming the launch's own subjects that `plan` doesn't list, as an earlier run
/// that crashed left them, and each one whose recorded item isn't at its path now (moved,
/// replaced, or none recorded), so the launch writes it again on the item there.
pub(super) fn drop_stale(
    ledger: &mut Ledger,
    plan: &AdmittedPlan,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> Result<(), Error> {
    let matter = plan.matter();
    let sids = plan.assigned_to(matter);
    let mine: Vec<Entry> = ledger
        .entries()
        .filter(|e| e.matter == matter && e.kind == PathKind::OtherMatterRead)
        .filter(|e| sids.contains(&e.sid))
        .cloned()
        .collect();
    let mut here: BTreeSet<EntryId> = BTreeSet::new();
    for entry in mine {
        let Some(file) = &entry.file else { continue };
        if acl.file_id(&entry.path).ok().flatten() == Some(file.id) {
            here.insert(entry.id);
        }
    }
    let listed = |entry: &Entry| here.contains(&entry.id) && plan.grants().contains(&entry.grant());
    // A deny lifted for a folder the plan no longer allows itself goes back first.
    let read = |e: &Entry| {
        let on = |g: &crate::windows::acl_plan::Grant| {
            g.kind() == PathKind::OtherMatterRead && same_path(g.path(), &e.path)
        };
        e.lifted == Some(matter) && sids.contains(&e.sid) && !plan.grants().iter().any(on)
    };
    let restored = lifts::restore(ledger, acl, read);
    finished(
        restored,
        take_reads(ledger, matter, &sids, listed, acl, around),
    )
    .map(drop)
}

/// Whether `a` and `b` are the same path, by [`in_folder`]'s comparison.
fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    in_folder(a, b) && in_folder(b, a)
}

/// Takes back each of `matter`'s read entries naming one of `sids`, in the file as read under
/// the lock, that `held` doesn't hold: removed, each tree walked again, then dropped.
fn take_reads(
    ledger: &mut Ledger,
    matter: MatterId,
    sids: &[Sid],
    held: impl Fn(&Entry) -> bool,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> Result<Narrowed, Error> {
    let kind = PathKind::OtherMatterRead;
    let failed = |e: io::Error| refused(kind, ApplyFailure::Ledger(e.kind()));
    let pick = |entry: &Entry| {
        let mine = entry.matter == matter && entry.kind == kind && sids.contains(&entry.sid);
        (mine && !held(entry)).then_some(Take::Remove)
    };
    let taken = take_back(ledger, None, acl, pick).map_err(failed)?;
    finish(ledger, acl, around, &taken, None, &[])
        .map_err(|refusal| Refused { kind, ..refusal }.error())?;
    if taken.kept > 0 {
        // A removal failed: its entry stays `removing`, and the allow may still be there.
        return Err(refused(kind, ApplyFailure::Io(io::ErrorKind::Other)));
    }
    let removed: BTreeSet<PathBuf> = taken.removed.into_iter().map(|e| e.path).collect();
    if !removed.is_empty() {
        let count = removed.len();
        tracing::info!(count, "tier B took back reads of other matters");
    }
    Ok(Narrowed {
        removed: removed.into_iter().collect(),
        restored: Vec::new(),
    })
}

/// Before `plan`'s allows are written: records on each read of another matter's entry this
/// launch added the item its path names now, by its file ID, so the take-back finds it wherever
/// it goes. An item missing now, or a link there, refuses as [`ApplyFailure::Changed`].
pub(super) fn record_files(
    ledger: &mut Ledger,
    plan: &AdmittedPlan,
    acl: &mut dyn AclWriter,
) -> Result<(), Refused> {
    let kind = PathKind::OtherMatterRead;
    for (grant, &id) in plan.grants().iter().zip(plan.entries()) {
        if grant.kind() != kind || !plan.created().contains(&id) {
            continue;
        }
        let file = match acl.file_id(grant.path()) {
            Ok(Some(file)) => RecordedFile::new(grant.path().to_owned(), file),
            Ok(None) => return Err(Refused::new(kind, ApplyFailure::Changed)),
            Err(error) => return Err(Refused::new(kind, ApplyFailure::of(&error))),
        };
        ledger
            .set_file(plan, id, &file)
            .map_err(|e| Refused::new(kind, ApplyFailure::Ledger(e.kind())))?;
    }
    Ok(())
}

/// Writes read `grant` (entry `id`) on the item its entry recorded, and nowhere else: one
/// recording none, or another item at the path now, is [`ApplyFailure::Changed`].
pub(super) fn add(
    ledger: &Ledger,
    id: Option<EntryId>,
    grant: &Grant,
    acl: &mut dyn AclWriter,
) -> Result<Added, ApplyFailure> {
    let entry = ledger.entries().find(|e| Some(e.id) == id);
    match entry.and_then(|e| e.file.as_ref()) {
        Some(file) => acl.add_recorded(grant, file),
        None => Err(ApplyFailure::Changed),
    }
}

/// Before any read of another matter is written (R18): the whole granted tree is held to the
/// strict walk's rule, watching the launch's own subjects besides `strict`'s. Each granted folder
/// has its children walked strictly; every folder of a read, at any depth, has allows of its own
/// in the plan, so every folder is walked and every item below the top is a child of one. The
/// top of each read (a folder or a file no other read holds) is checked on its own
/// ([`AclWriter::check_item`]), unless it is a matter's root, which carries the fence's explicit
/// deny on itself, first in its DACL. An item with inheritance off (which the fence's deny
/// doesn't reach), or with an explicit allow that beats the deny, refuses the launch, naming it.
/// The reads the ledger lists are excused, as everywhere (`strict.excused`). Nothing is set
/// aside: the tree is the other matter's. What the walks saw.
pub(super) fn check_trees(
    ledger: &Ledger,
    plan: &AdmittedPlan,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
    strict: &Strict,
) -> Result<Walked, Refused> {
    let kind = PathKind::OtherMatterRead;
    let paths: BTreeSet<&Path> = plan
        .grants()
        .iter()
        .filter(|g| g.kind() == kind)
        .map(Grant::path)
        .collect();
    let mut seen = Walked::default();
    if paths.is_empty() {
        return Ok(seen);
    }
    let mut watched: BTreeSet<Sid> = strict.watched.iter().cloned().collect();
    watched.extend(plan.assigned_to(plan.matter()));
    let check = Strict {
        watched: watched.into_iter().collect(),
        assigned: Vec::new(),
        ..strict.clone()
    };
    // A matter's root: the fence's deny on the folder alone, planned now or listed.
    let root_deny = |k: PathKind, r: Rights, i: Inherit| {
        k == PathKind::Matter && r == Rights::DenyReadWrite && i == Inherit::ThisOnly
    };
    let planned = plan.grants().iter();
    let planned = planned.filter(|g| root_deny(g.kind(), g.rights(), g.inherit()));
    let listed = ledger
        .entries()
        .filter(|e| root_deny(e.kind, e.rights, e.inherit));
    let roots: Vec<&Path> = planned
        .map(Grant::path)
        .chain(listed.map(|e| e.path.as_path()))
        .collect();
    let top = |p: &&Path| !paths.iter().any(|f| f != p && in_folder(p, f));
    for item in paths.iter().copied().filter(top) {
        if roots.iter().any(|root| same_path(root, item)) {
            continue;
        }
        acl.check_item(item, &check).map_err(|failure| Refused {
            at: Some(item.to_path_buf()),
            ..Refused::new(kind, failure)
        })?;
    }
    for folder in paths.iter().filter(|p| is_folder(p)) {
        let walked = clearing(acl, around, &[], kind, |acl| {
            acl.propagate(folder, Reach::Children, Some(&check))
        })?;
        seen.add(walked);
    }
    Ok(seen)
}

/// Whether `path` is a folder, not followed if a link.
fn is_folder(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}
