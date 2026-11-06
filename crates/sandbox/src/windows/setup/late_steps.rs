//! Set-up's steps around the matters: reading the owner's registered matters after the lock
//! (`PR4h`), step 8's fence of them against the grown pool, and step 9's change journals.

use std::io;
use std::path::{Path, PathBuf};

use super::create::Failed;
use super::journal_plan::{JournalAction, journal_plan};
use super::machine::{Machine, owned};
use super::outcome::{JournalOutcome, SetupFailure, Step, cause_of};
use super::rollback::Run;
use crate::windows::apply::{Applied, fence_registered};
use crate::windows::fence::RegisteredMatter;
use crate::windows::pool::MachineRecord;
use crate::windows::sids::Sid;
use crate::windows::writer::AclWriter;

/// The owner's registered matters (`Machine::registered_matters`), read as the owner once the
/// lock is held, so no other set-up changes the record, the ledger or the pool between this read
/// and the fence. The record's pool users and group are trusted in the ledger. A file that
/// can't be read fails closed at [`Step::Matters`]; a missing one is no matters.
pub(super) fn registered(
    machine: &mut impl Machine,
    record: Option<&MachineRecord>,
) -> Result<Vec<RegisteredMatter>, SetupFailure> {
    let mut trusted: Vec<Sid> = record.map(|r| r.pool.clone()).unwrap_or_default();
    trusted.extend(record.map(|r| r.group.clone()));
    owned(machine, |m| m.registered_matters(&trusted)).map_err(|error| {
        SetupFailure::early(
            Step::Matters,
            format!("the registered matters: {}", cause_of(&error)),
        )
    })
}

/// Step 8: when this run made users and matters are registered (an extend, or a re-run that made
/// a user again), every registered matter is fenced against the grown pool through the owner's
/// ledger, which the rollback reads after. The whole step runs as the owner: the ledger is the
/// owner's file, and the matters' folders are the owner's, so the elevated process writes there
/// only what the owner could (K1).
pub(super) fn fence(
    machine: &mut impl Machine,
    acl: &mut impl AclWriter,
    matters: &[RegisteredMatter],
    run: &mut Run,
    group: &Sid,
    live: &[Sid],
) -> Result<Option<Applied>, Failed> {
    if run.made.is_empty() || matters.is_empty() {
        return Ok(None);
    }
    let mut trusted: Vec<Sid> = run.recorded().map(|r| r.pool.clone()).unwrap_or_default();
    trusted.push(group.clone());
    let failed = |cause: String| (Step::Fence, cause);
    let (ledger, fenced) = machine
        .as_owner(|machine| {
            let mut ledger = machine.ledger(&trusted)?;
            let around = machine.surroundings();
            let fenced = fence_registered(&mut ledger, matters, live, acl, around);
            Ok::<_, io::Error>((ledger, fenced))
        })
        .map_err(|error| failed(cause_of(&error)))?
        .map_err(|error| failed(format!("the permission ledger: {error}")))?;
    run.ledger = Some(ledger);
    fenced.map(Some).map_err(|error| failed(error.to_string()))
}

/// Step 9: each chosen volume's change journal made [`super::JOURNAL_MAX`] when smaller
/// ([`journal_plan`], D-B12).
/// Failures are listed, never fatal: they cost only full walks later.
pub(super) fn journals(
    machine: &mut impl Machine,
    matters: &[RegisteredMatter],
) -> Result<Vec<(PathBuf, JournalOutcome)>, String> {
    let roots: Vec<&Path> = matters
        .iter()
        .map(|matter| matter.folders.root.as_path())
        .collect();
    let volumes = machine
        .journal_volumes(&roots)
        .map_err(|error| cause_of(&error))?;
    let mut done = Vec::new();
    for volume in volumes {
        let outcome = match machine.journal_size(&volume) {
            Ok(size) => match journal_plan(size) {
                JournalAction::Leave => JournalOutcome::Left(size.unwrap_or_default()),
                JournalAction::Grow { max, delta } => {
                    match machine.grow_journal(&volume, max, delta) {
                        Ok(()) => JournalOutcome::Grown,
                        Err(error) => JournalOutcome::Failed(cause_of(&error)),
                    }
                }
            },
            Err(error) => JournalOutcome::Failed(cause_of(&error)),
        };
        if let JournalOutcome::Failed(why) = &outcome {
            tracing::warn!(volume = %volume.display(), %why, "the change journal wasn't enlarged");
        }
        done.push((volume, outcome));
    }
    Ok(done)
}
