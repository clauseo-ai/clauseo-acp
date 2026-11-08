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
//! Each call takes back only the entries naming the session's own SIDs (`sids`: its pool user,
//! and the matter's SID when that is a subject, off by default), so a second session of the
//! matter, with a pool user of its own, keeps its reads. The matter's SID is the matter's, not
//! one session's: with it on, any session's end takes its reads back for both, the safe way.
//!
//! [`forget_matter`]: super::forget_matter

use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;

use clauseo_protocol::MatterId;

use super::back::{finish, take_back};
use super::lifts;
use super::walk::Refused;
use super::{Surroundings, refused};
use crate::windows::acl_plan::{AdmittedPlan, Sid, in_folder};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{Entry, Ledger, Take};
use crate::windows::writer::{AclWriter, ApplyFailure};
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
/// rest. Idempotent: a second call with the same policy takes nothing. A handle the agent
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
/// that crashed left them.
pub(super) fn drop_stale(
    ledger: &mut Ledger,
    plan: &AdmittedPlan,
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
) -> Result<(), Error> {
    let matter = plan.matter();
    let listed = |entry: &Entry| plan.grants().contains(&entry.grant());
    let sids = plan.assigned_to(matter);
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
