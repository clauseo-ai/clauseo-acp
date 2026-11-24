//! Turns of different threads in one matter run at the same time: a turn that passes step 3
//! joins its matter's running turns (`MatterTurns::running`) and leaves them as it ends. Each
//! running turn keeps what it knows of the others: whether another turn ran beside it, and the
//! papers the cards of those that ended meanwhile credit.
//!
//! - A before-turn checkpoint taken while turns run leaves out of its outside edits the papers
//!   their tool calls name ([`named_by_others`]): those are their work, not the lawyer's edits
//!   outside Clauseo.
//! - The after-turn checkpoint of a turn that ran beside others is measured from its own
//!   before-turn checkpoint, without the papers only they named or were credited
//!   ([`overlap_of`]). A turn that ran alone is checkpointed as it always was.
//!
//! What the matter's thread makes of the places is in `matter_worker/beside.rs`.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_protocol::{MatterId, MatterPath, ThreadId};

use super::super::Actor;
use super::close::TurnClose;
use super::current;
use crate::log_file::LIFECYCLE;
use crate::matter_worker::TurnOverlap;
use crate::sent::TurnPaths;

/// What a turn knows of the other turns of its matter that ran beside it.
#[derive(Debug, Default)]
pub(super) struct Overlap {
    /// Another turn of the matter was running at its step 3, or passed step 3 while it ran.
    pub(super) overlapped: bool,
    /// The papers the cards of turns that ended while it ran credit, with those turns' threads.
    pub(super) others_ended: BTreeMap<MatterPath, BTreeSet<ThreadId>>,
    /// Its tool calls' places as its end took them for its after-turn checkpoint, which the
    /// turns running beside it read until it leaves the matter.
    pub(super) ending: Option<TurnPaths>,
}

/// `thread`'s turn passes step 3 in `matter`: it runs beside every turn already running there,
/// and each of those beside it.
pub(super) fn join(actor: &mut Actor, matter: MatterId, thread: ThreadId) {
    let Some(entry) = actor.matters.get_mut(&matter) else {
        return;
    };
    let others: Vec<ThreadId> = entry
        .turns
        .running
        .iter()
        .copied()
        .filter(|other| *other != thread)
        .collect();
    entry.turns.running.insert(thread);
    if others.is_empty() {
        return;
    }
    for running in others.into_iter().chain([thread]) {
        if let Some(current) = current(actor, running) {
            current.overlap.overlapped = true;
        }
    }
}

/// The papers `close`'s card credits to its turn: the files it changed with no flag for the
/// lawyer's edits.
pub(super) fn credited(close: &TurnClose) -> Vec<MatterPath> {
    close
        .files
        .iter()
        .flat_map(|changed| &changed.files)
        .filter(|file| !file.may_include_lawyer_edits())
        .map(|file| file.path().clone())
        .collect()
}

/// `thread`'s turn ends with `credited` on its card: each other turn running in `matter` keeps
/// those papers, with `thread`.
pub(super) fn ended(
    actor: &mut Actor,
    matter: MatterId,
    thread: ThreadId,
    credited: &[MatterPath],
) {
    if credited.is_empty() {
        return;
    }
    let others: Vec<ThreadId> = actor
        .matters
        .get(&matter)
        .map(|entry| {
            let running = entry.turns.running.iter().copied();
            running.filter(|other| *other != thread).collect()
        })
        .unwrap_or_default();
    for other in others {
        if let Some(current) = current(actor, other) {
            for path in credited {
                let threads = current
                    .overlap
                    .others_ended
                    .entry(path.clone())
                    .or_default();
                threads.insert(thread);
            }
        }
    }
}

/// What `thread`'s after-turn checkpoint needs of the turns that ran beside it in `matter`:
/// `None` for a turn that ran alone, or whose before-turn checkpoint is not known, which is
/// measured from the last checkpoint as before.
pub(super) fn overlap_of(
    actor: &mut Actor,
    matter: MatterId,
    thread: ThreadId,
) -> Option<Box<TurnOverlap>> {
    let running = running_beside(actor, matter, thread);
    let current = current(actor, thread)?;
    if !current.overlap.overlapped {
        return None;
    }
    Some(Box::new(TurnOverlap {
        since: current.checkpoint_before?,
        running,
        ended: current.overlap.others_ended.clone(),
    }))
}

/// The other turns running in `matter` than `thread`'s, with their tool calls' places.
fn running_beside(actor: &Actor, matter: MatterId, thread: ThreadId) -> Vec<(ThreadId, TurnPaths)> {
    let Some(entry) = actor.matters.get(&matter) else {
        return Vec::new();
    };
    entry
        .turns
        .running
        .iter()
        .filter(|other| **other != thread)
        .filter_map(|other| {
            let current = actor.threads.get(other)?.turn.current.as_deref()?;
            let paths = current
                .overlap
                .ending
                .as_ref()
                .unwrap_or(&current.tool_paths);
            Some((*other, paths.clone()))
        })
        .collect()
}

/// The tool-call places of the turns running in `matter` other than `thread`'s, for its
/// before-turn checkpoint; the matter's thread resolves them as it resolves a turn's own for
/// its after-turn checkpoint (`matter_worker::tool_paths`).
pub(super) fn named_by_others(actor: &Actor, matter: MatterId, thread: ThreadId) -> Vec<TurnPaths> {
    let running = running_beside(actor, matter, thread);
    running.into_iter().map(|(_, paths)| paths).collect()
}

/// Logs that `thread`'s ending turn ran beside others of its matter, with how many papers
/// their cards credited meanwhile; nothing for a turn that ran alone.
pub(super) fn log_overlap(thread: ThreadId, overlap: &Overlap) {
    if overlap.overlapped {
        let credited = overlap.others_ended.len();
        tracing::info!(target: LIFECYCLE, %thread, credited, "a turn ran beside another of its matter");
    }
}
