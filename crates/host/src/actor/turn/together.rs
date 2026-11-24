//! Turns of different threads in one matter run at the same time: a turn that passes step 3
//! joins its matter's running turns (`MatterTurns::running`) and leaves them as it ends.
//!
//! Two turns ran beside each other when their before- and after-turn checkpoints interleave.
//! The matter's thread runs those jobs in the order they are sent, so each turn notes where its
//! two came in the matter's count of them (`MatterTurns::sent`): a turn whose after-turn
//! checkpoint was sent before another's before-turn checkpoint ran before it, as two turns one
//! after the other always did. Each running turn keeps the threads it ran beside, and the papers
//! the cards of those that ended meanwhile credit, with the content each card left them with.
//!
//! - A before-turn checkpoint leaves out of its outside edits the papers the tool calls of the
//!   turns still before their after-turn checkpoint name ([`named_by_others`]): those are their
//!   work, not the lawyer's edits outside Clauseo.
//! - The after-turn checkpoint of a turn that ran beside others is measured from its own
//!   before-turn checkpoint, without the papers only they named or were credited
//!   ([`overlap_of`]). A turn that ran beside none is checkpointed as it always was.
//!
//! What the matter's thread makes of the places is in `matter_worker/beside.rs`.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_protocol::{ChangeKind, ContentId, MatterId, MatterPath, ThreadId};

use super::super::Actor;
use super::close::TurnClose;
use super::current;
use crate::log_file::LIFECYCLE;
use crate::matter_worker::{Ended, TurnOverlap};
use crate::sent::TurnPaths;

/// What a turn knows of the other turns of its matter that ran beside it.
#[derive(Debug, Default)]
pub(super) struct Overlap {
    /// Where its before-turn checkpoint (the last one, after a relaunch) came in its matter's
    /// count of turn checkpoints sent.
    before: Option<u64>,
    /// Where its after-turn checkpoint came in that count.
    after: Option<u64>,
    /// The threads whose turns ran beside it, as they were then: a later turn of one of them ran
    /// beside it only if that turn has it in its own (see [`paired`]).
    beside: BTreeSet<ThreadId>,
    /// The papers the cards of turns that ran beside it and ended meanwhile credit.
    ended: BTreeMap<MatterPath, Ended>,
    /// The turns that ran beside it and ended with no card (their after-turn checkpoint or its
    /// events failed), with their tool calls' places.
    failed: Vec<(ThreadId, TurnPaths)>,
    /// Its tool calls' places as its end took them for its after-turn checkpoint, which the
    /// turns running beside it read until it leaves the matter.
    pub(super) ending: Option<TurnPaths>,
    /// The papers its card credits it with, set once its after-turn checkpoint is in its card.
    card: Option<Vec<(MatterPath, Option<ContentId>)>>,
}

/// `thread`'s turn passes step 3 in `matter`: it joins the turns running there.
pub(super) fn join(actor: &mut Actor, matter: MatterId, thread: ThreadId) {
    if let Some(entry) = actor.matters.get_mut(&matter) {
        entry.turns.running.insert(thread);
    }
}

/// The next number in `matter`'s count of turn checkpoints sent.
fn count(actor: &mut Actor, matter: MatterId) -> Option<u64> {
    let turns = &mut actor.matters.get_mut(&matter)?.turns;
    turns.sent += 1;
    Some(turns.sent)
}

/// `thread`'s before-turn checkpoint has been sent to `matter`'s thread: its turn runs beside each
/// running turn whose before-turn checkpoint was sent and whose after-turn checkpoint was not.
/// Sent again after a relaunch, it starts over from there: what ended meanwhile is in this one.
pub(super) fn before_sent(actor: &mut Actor, matter: MatterId, thread: ThreadId) {
    let Some(at) = count(actor, matter) else {
        return;
    };
    let others: Vec<ThreadId> = actor
        .matters
        .get(&matter)
        .map(|entry| {
            let running = entry.turns.running.iter().copied();
            running.filter(|other| *other != thread).collect()
        })
        .unwrap_or_default();
    let mut beside = BTreeSet::new();
    for other in others {
        if let Some(running) = current(actor, other)
            && running.overlap.before.is_some()
            && running.overlap.after.is_none()
        {
            running.overlap.beside.insert(thread);
            beside.insert(other);
        }
    }
    if let Some(own) = current(actor, thread) {
        own.overlap = Overlap {
            before: Some(at),
            beside,
            ..Overlap::default()
        };
    }
}

/// `thread`'s after-turn checkpoint has been sent to `matter`'s thread: no turn whose before-turn
/// checkpoint comes later ran beside it.
pub(super) fn after_sent(actor: &mut Actor, matter: MatterId, thread: ThreadId) {
    let at = count(actor, matter);
    if let Some(own) = current(actor, thread) {
        own.overlap.after = at;
    }
}

/// Whether the current turns of `a` and `b` ran beside each other.
fn paired(actor: &Actor, a: ThreadId, b: ThreadId) -> bool {
    let beside = |of: ThreadId, other: ThreadId| {
        let current = actor
            .threads
            .get(&of)
            .and_then(|entry| entry.turn.current.as_deref());
        current.is_some_and(|current| current.overlap.beside.contains(&other))
    };
    beside(a, b) && beside(b, a)
}

/// The papers `close`'s card credits to its turn, with the content it left each with: the files
/// it changed with no flag for the lawyer's edits, and the old path of each rename, left empty.
fn credited(close: &TurnClose) -> Vec<(MatterPath, Option<ContentId>)> {
    let mut credited = Vec::new();
    let files = close.files.iter().flat_map(|changed| &changed.files);
    for file in files.filter(|file| !file.may_include_lawyer_edits()) {
        credited.push((file.path().clone(), file.after().cloned()));
        if let ChangeKind::Renamed { from } = file.change() {
            credited.push((from.clone(), None));
        }
    }
    credited
}

/// `thread`'s after-turn checkpoint and its events are in `close`, its card: the turns running
/// beside it keep what the card credits once it ends.
pub(super) fn carded(actor: &mut Actor, thread: ThreadId, close: &TurnClose) {
    let card = credited(close);
    if let Some(own) = current(actor, thread) {
        own.overlap.card = Some(card);
    }
}

/// `thread`'s turn ends: each turn running beside it in `matter` keeps the papers its card
/// credits, or, when it has no card (its after-turn checkpoint or that checkpoint's events
/// failed), its tool calls' places: those papers stay on that turn's card, naming it, since
/// they are on no other.
pub(super) fn ended(actor: &mut Actor, matter: MatterId, thread: ThreadId) {
    let Some(own) = current(actor, thread) else {
        return;
    };
    let card = own.overlap.card.take();
    let places = own.overlap.ending.clone();
    let beside: Vec<ThreadId> = own.overlap.beside.iter().copied().collect();
    let running = actor
        .matters
        .get(&matter)
        .map(|entry| entry.turns.running.clone())
        .unwrap_or_default();
    for other in beside {
        if !running.contains(&other) || !paired(actor, thread, other) {
            continue;
        }
        let Some(running) = current(actor, other) else {
            continue;
        };
        match (&card, &places) {
            (Some(card), _) => {
                for (path, left) in card {
                    let ended = running.overlap.ended.entry(path.clone()).or_default();
                    ended.threads.insert(thread);
                    ended.left.clone_from(left);
                }
            }
            (None, Some(places)) => running.overlap.failed.push((thread, places.clone())),
            (None, None) => {}
        }
    }
}

/// What `thread`'s after-turn checkpoint needs of the turns that ran beside it in `matter`:
/// `None` for a turn that ran beside none, or whose before-turn checkpoint is not known, which is
/// measured from the last checkpoint as before.
pub(super) fn overlap_of(
    actor: &mut Actor,
    matter: MatterId,
    thread: ThreadId,
) -> Option<Box<TurnOverlap>> {
    let running = running_beside(actor, matter, thread);
    let own = current(actor, thread)?;
    if own.overlap.beside.is_empty() {
        return None;
    }
    Some(Box::new(TurnOverlap {
        since: own.checkpoint_before?,
        running,
        failed: own.overlap.failed.clone(),
        ended: own.overlap.ended.clone(),
    }))
}

/// The turns running in `matter` that ran beside `thread`'s, with their tool calls' places.
fn running_beside(actor: &Actor, matter: MatterId, thread: ThreadId) -> Vec<(ThreadId, TurnPaths)> {
    let Some(entry) = actor.matters.get(&matter) else {
        return Vec::new();
    };
    entry
        .turns
        .running
        .iter()
        .filter(|other| **other != thread && paired(actor, thread, **other))
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

/// The tool-call places of the turns running in `matter` other than `thread`'s whose after-turn
/// checkpoint has not been sent, for its before-turn checkpoint; the matter's thread resolves them
/// as it resolves a turn's own for its after-turn checkpoint (`matter_worker::tool_paths`). One
/// whose after-turn checkpoint was sent is left out: the matter's thread runs that checkpoint
/// first, and it holds that turn's writes.
pub(super) fn named_by_others(actor: &Actor, matter: MatterId, thread: ThreadId) -> Vec<TurnPaths> {
    let Some(entry) = actor.matters.get(&matter) else {
        return Vec::new();
    };
    entry
        .turns
        .running
        .iter()
        .filter(|other| **other != thread)
        .filter_map(|other| actor.threads.get(other)?.turn.current.as_deref())
        .filter(|current| current.overlap.after.is_none())
        .map(|current| {
            let paths = current.overlap.ending.as_ref();
            paths.unwrap_or(&current.tool_paths).clone()
        })
        .collect()
}

/// Logs that `thread`'s ending turn ran beside others of its matter, with how many papers
/// their cards credited meanwhile; nothing for a turn that ran beside none.
pub(super) fn log_overlap(thread: ThreadId, overlap: &Overlap) {
    if !overlap.beside.is_empty() {
        let (beside, credited) = (overlap.beside.len(), overlap.ended.len());
        let message = "a turn ran beside others of its matter";
        tracing::info!(target: LIFECYCLE, %thread, beside, credited, "{message}");
    }
}
