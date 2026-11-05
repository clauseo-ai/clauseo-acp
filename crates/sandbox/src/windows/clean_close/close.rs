//! The session end's side: the cursor taken before the repair, and the clean-close record
//! written only when the session ended clean.

use std::io;
use std::path::PathBuf;

use clauseo_protocol::MatterId;

use super::WalkInputs;
use super::check::{Busy, busy};
use super::journal::{Journal, Usn};
use super::record::{self, CleanRecord, FOLDER, Header, Item, Known, ROOT, RootMark, VolumeMark};
use crate::Error;
use crate::windows::admit::PathKind;
use crate::windows::ledger::Ledger;
use crate::windows::set_aside::{self, Checked};
use crate::windows::writer::Walked;

/// Each own root and each volume's journal position, taken before the session-end repair
/// starts, so a change made while it runs is in the journal past the cursor and the next
/// launch looks at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    matter: MatterId,
    roots: Vec<RootMark>,
    /// Each volume's serial, journal ID and next USN.
    volumes: Vec<(u64, u64, Usn)>,
}

/// Whether the record was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    /// The record was written: the next launch may skip.
    Written,
    /// It wasn't, for this reason: the next launch walks in full.
    NotWritten(NotWritten),
}

/// Why no record was written. New reasons are appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotWritten {
    /// No cursor was taken (no usable journal, a root on a removable drive), or another
    /// matter's was given.
    NoCursor,
    /// The session-end repair failed.
    RepairFailed,
    /// The after-session pass failed or its scan was partial.
    ScanIncomplete,
    /// A session of the matter is open.
    SessionOpen,
    /// A ledger entry on, in or above an own root is `pending` or `removing`.
    LedgerBusy,
    /// A hand-back is owed for the matter.
    HandbackOwed,
    /// The own roots aren't the ones the cursor was taken for, or one is another folder now.
    RootsChanged,
    /// A tree's file IDs couldn't all be read.
    TreeUnreadable,
    /// The record couldn't be written, of this kind.
    WriteFailed(io::ErrorKind),
}

/// The cursor for `matter`'s own trees (its ledger entries, as `repair_after_session` reads
/// them), taken just before that repair starts. `None` when any root can't be read, is on a
/// removable drive or a volume that isn't NTFS, or its journal can't be queried: the next launch
/// would walk in full anyway.
pub fn session_cursor(ledger: &Ledger, matter: MatterId, journal: &dyn Journal) -> Option<Cursor> {
    let inputs = WalkInputs::of_ledger(ledger, matter);
    let mut roots = Vec::new();
    let mut volumes: Vec<(u64, u64, Usn)> = Vec::new();
    for (path, kind) in &inputs.roots {
        let facts = journal.root(path).ok()?;
        if facts.removable || !facts.ntfs {
            return None;
        }
        roots.push(RootMark {
            path: path.clone(),
            kind: *kind,
            volume: facts.volume,
            id: facts.id?,
        });
        if !volumes.iter().any(|v| v.0 == facts.volume) {
            let state = journal.query(path).ok()?;
            volumes.push((facts.volume, state.id, state.next));
        }
    }
    (!roots.is_empty()).then_some(Cursor {
        matter,
        roots,
        volumes,
    })
}

/// Writes `matter`'s clean-close record, once the agent has exited, after `repair` (what
/// `repair_after_session` returned) and `after` (what `after_session` returned), only when the
/// session ended clean: both passed and the scan was complete, no session of the matter is open
/// (checked under the set-aside lock the record is written under, so a launch that started
/// meanwhile wins), no ledger entry near an own root is `pending` or `removing`, no hand-back
/// is owed, and each root is still the folder the cursor was taken for. The trees' file IDs are
/// read now ([`Journal::tree`]); anything that changes after the cursor is in the journal.
/// `ledger` is read as the caller opened it, after the repair.
pub fn write_clean_close(
    data_dir: &std::path::Path,
    ledger: &Ledger,
    matter: MatterId,
    journal: &dyn Journal,
    cursor: Option<&Cursor>,
    repair: &Result<Walked, Error>,
    after: &Result<Checked, Error>,
) -> Closed {
    let closed = write(data_dir, ledger, matter, journal, cursor, repair, after);
    match closed {
        Ok(()) => {
            tracing::info!("tier B kept the clean-close record");
            Closed::Written
        }
        Err(why) => {
            tracing::info!(?why, "tier B kept no clean-close record");
            Closed::NotWritten(why)
        }
    }
}

fn write(
    data_dir: &std::path::Path,
    ledger: &Ledger,
    matter: MatterId,
    journal: &dyn Journal,
    cursor: Option<&Cursor>,
    repair: &Result<Walked, Error>,
    after: &Result<Checked, Error>,
) -> Result<(), NotWritten> {
    let cursor = cursor
        .filter(|c| c.matter == matter)
        .ok_or(NotWritten::NoCursor)?;
    if repair.is_err() {
        return Err(NotWritten::RepairFailed);
    }
    if !after.as_ref().is_ok_and(|checked| !checked.partial()) {
        return Err(NotWritten::ScanIncomplete);
    }
    let inputs = WalkInputs::of_ledger(ledger, matter);
    let marked: Vec<(PathBuf, PathKind)> = cursor
        .roots
        .iter()
        .map(|r| (r.path.clone(), r.kind))
        .collect();
    if inputs.roots != marked {
        return Err(NotWritten::RootsChanged);
    }
    match busy(ledger, matter, &inputs) {
        Some(Busy::Ledger) => return Err(NotWritten::LedgerBusy),
        Some(Busy::Handback) => return Err(NotWritten::HandbackOwed),
        None => {}
    }
    let mut by_volume: Vec<Vec<Item>> = vec![Vec::new(); cursor.volumes.len()];
    for root in &cursor.roots {
        let facts = journal
            .root(&root.path)
            .map_err(|_unread| NotWritten::RootsChanged)?;
        if facts.volume != root.volume || facts.id != Some(root.id) {
            return Err(NotWritten::RootsChanged);
        }
        let slot = cursor
            .volumes
            .iter()
            .position(|v| v.0 == root.volume)
            .and_then(|at| by_volume.get_mut(at))
            .ok_or(NotWritten::RootsChanged)?;
        let listed = journal
            .tree(&root.path)
            .map_err(|_unread| NotWritten::TreeUnreadable)?;
        slot.push(Item {
            id: root.id,
            parent: 0,
            flags: FOLDER | ROOT,
        });
        slot.extend(listed.into_iter().map(|l| Item {
            id: l.id,
            parent: l.parent,
            flags: if l.folder { FOLDER } else { 0 },
        }));
    }
    let known: Vec<Known> = by_volume.into_iter().map(Known::new).collect();
    let volumes = cursor
        .volumes
        .iter()
        .zip(&known)
        .map(|(&(volume, journal, cursor), known)| VolumeMark {
            volume,
            journal,
            cursor,
            items: known.len(),
        })
        .collect();
    let record = CleanRecord {
        header: Header {
            matter,
            inputs,
            roots: cursor.roots.clone(),
            volumes,
        },
        known,
    };
    let written = set_aside::under_lock(data_dir, &matter, |open| {
        if open {
            return Err(NotWritten::SessionOpen);
        }
        record::write(data_dir, &record).map_err(|e| NotWritten::WriteFailed(e.kind()))
    });
    written.map_err(|_unlocked| NotWritten::WriteFailed(io::ErrorKind::WouldBlock))?
}
