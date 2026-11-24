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
//!   before-turn checkpoint, without the papers only they wrote, were credited, or name in
//!   calls still unfinished ([`overlap_of`]). A turn that ran beside none is checkpointed as it
//!   always was.
//! - A paper a checkpoint left out as a running turn's (out of a before-turn checkpoint's
//!   outside edits, or off an after-turn checkpoint's card) is in that checkpoint already, so no
//!   later checkpoint logs it. When that turn then ends with no card of its own, the paper is
//!   logged as an outside edit on that checkpoint in its end's commit ([`log_absorbed`]), so it
//!   stays on the record and undoable as any outside edit is. The turns still running beside it
//!   leave it off their cards while it holds what was logged.
//!
//! What the matter's thread makes of the places is in `matter_worker/beside.rs`; it reads a
//! running turn's places when the job runs (`sent/shared.rs`).

use std::collections::{BTreeMap, BTreeSet};

use clauseo_protocol::{
    ChangeKind, CheckpointId, ContentId, DomainEvent, FileChange, MatterId, MatterPath,
    OutsideEdits, StreamId, ThreadId, TurnId, ViewEntry,
};

use super::super::Actor;
use super::close::TurnClose;
use super::current;
use crate::log_file::LIFECYCLE;
use crate::matter_worker::{Ended, TurnOverlap};
use crate::pipeline::MatterEvent;
use crate::sent::{SharedPaths, TurnPaths};

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
    pub(super) ending: Option<SharedPaths>,
    /// The turns running as its last checkpoint job was sent, whose papers that checkpoint
    /// leaves out (of its outside edits, or of its card), each with its turn.
    measured: BTreeMap<ThreadId, TurnId>,
    /// The changes of its papers other turns' checkpoints left out as its (of their outside
    /// edits, or off their cards), by checkpoint: logged as outside edits if it ends with no card.
    absorbed: BTreeMap<CheckpointId, Vec<FileChange>>,
    /// The papers its end logged of `absorbed`, with the content logged, which the turns running
    /// beside it leave off their cards while they hold it.
    logs: Vec<(MatterPath, Option<ContentId>)>,
    /// The papers turns that ran beside it and ended with no card logged as outside edits, with
    /// the content logged: left off its card while they hold it, being on the record already.
    logged: BTreeMap<MatterPath, Option<ContentId>>,
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
    let measured = named_by_others(actor, matter, thread)
        .into_iter()
        .filter_map(|(other, _)| Some((other, current_turn(actor, other)?)))
        .collect();
    if let Some(own) = current(actor, thread) {
        own.overlap = Overlap {
            before: Some(at),
            beside,
            measured,
            ..Overlap::default()
        };
    }
}

/// The turn `thread` runs now.
fn current_turn(actor: &Actor, thread: ThreadId) -> Option<TurnId> {
    Some(actor.threads.get(&thread)?.turn.current.as_deref()?.turn)
}

/// `thread`'s before-turn checkpoint answered: each change it left out of its outside edits as
/// a running turn's is that turn's to log should it end with no card; one of a turn that ended
/// meanwhile is an outside edit of this checkpoint after all, put back among `changes`.
pub(super) fn before_answered(
    actor: &mut Actor,
    matter: MatterId,
    thread: ThreadId,
    checkpoint: &mut clauseo_matters::Checkpoint,
) {
    let left_out = std::mem::take(&mut checkpoint.left_out);
    let measured = current(actor, thread)
        .map(|own| std::mem::take(&mut own.overlap.measured))
        .unwrap_or_default();
    if left_out.is_empty() {
        return;
    }
    let gone = absorb(actor, matter, &measured, checkpoint.outcome.id(), left_out);
    if !gone.is_empty() {
        checkpoint.changes.extend(gone);
        checkpoint.changes.sort_by(|a, b| {
            a.path()
                .as_str()
                .as_bytes()
                .cmp(b.path().as_str().as_bytes())
        });
    }
}

/// Hands each change of `left_out`, which checkpoint `checkpoint` left out as the turns it names
/// in `also_changed_by`, to those of them `measured` holds that still run the same turn, and
/// returns, as outside edits, those of a turn that ended meanwhile and no running one holds.
fn absorb(
    actor: &mut Actor,
    matter: MatterId,
    measured: &BTreeMap<ThreadId, TurnId>,
    checkpoint: CheckpointId,
    left_out: Vec<FileChange>,
) -> Vec<FileChange> {
    let running = actor
        .matters
        .get(&matter)
        .map(|entry| entry.turns.running.clone())
        .unwrap_or_default();
    let mut gone_edits = Vec::new();
    for change in left_out {
        let (mut held, mut gone) = (false, false);
        for other in change.also_changed_by() {
            let Some(turn) = measured.get(other) else {
                continue;
            };
            let found = current(actor, *other)
                .filter(|current| running.contains(other) && current.turn == *turn);
            match found {
                Some(current) => {
                    let absorbed = current.overlap.absorbed.entry(checkpoint).or_default();
                    absorbed.push(plain(&change));
                    held = true;
                }
                None => gone = true,
            }
        }
        if gone && !held {
            gone_edits.push(plain(&change));
        }
    }
    gone_edits
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
/// beside it keep what the card credits once it ends. Each change `left_out` of the card as a
/// turn's that ran beside it is that turn's to log should it end with no card; one of a turn
/// that has ended meanwhile (it had no checkpoint either) is logged now, in `close`.
pub(super) fn carded(
    actor: &mut Actor,
    (matter, thread): (MatterId, ThreadId),
    close: &mut TurnClose,
    left_out: Vec<FileChange>,
) {
    let card = credited(close);
    let Some(own) = current(actor, thread) else {
        return;
    };
    own.overlap.card = Some(card);
    let measured = std::mem::take(&mut own.overlap.measured);
    let Some(checkpoint) = close.checkpoint_after else {
        return;
    };
    let gone = absorb(actor, matter, &measured, checkpoint, left_out);
    if !gone.is_empty() {
        push_edits(actor, matter, close, checkpoint, gone);
    }
}

/// `thread`'s turn ends with no card (its after-turn checkpoint or that checkpoint's events
/// failed): the changes of its papers the checkpoints of turns that ended before it took are
/// logged in `close`, its end's commit, as outside edits on those checkpoints. The turns running
/// beside it leave them off their cards while they hold what was logged ([`ended`]).
pub(super) fn log_absorbed(
    actor: &mut Actor,
    matter: MatterId,
    thread: ThreadId,
    close: &mut TurnClose,
) {
    let Some(own) = current(actor, thread) else {
        return;
    };
    let absorbed = std::mem::take(&mut own.overlap.absorbed);
    let files = absorbed.values().flatten();
    own.overlap.logs = files.flat_map(paths_of).collect();
    for (checkpoint, files) in absorbed {
        push_edits(actor, matter, close, checkpoint, files);
    }
}

/// The papers `change` touches, with what each holds after it: a rename's old path, nothing.
fn paths_of(change: &FileChange) -> Vec<(MatterPath, Option<ContentId>)> {
    let mut paths = vec![(change.path().clone(), change.after().cloned())];
    if let ChangeKind::Renamed { from } = change.change() {
        paths.push((from.clone(), None));
    }
    paths
}

/// `change` as an outside edit: neither flagged nor naming anyone.
fn plain(change: &FileChange) -> FileChange {
    FileChange::new(
        change.path().clone(),
        change.change().clone(),
        change.before().cloned(),
        change.after().cloned(),
        change.size_bytes(),
    )
    .unwrap_or_else(|_| change.clone().with_also_changed_by(Vec::new()))
}

/// Whether the end of `thread`'s turn, whose after-turn checkpoint answered `result`, may log
/// outside edits on a checkpoint of another answer ([`push_edits`]): it holds changes such a
/// checkpoint took, logged should it end with no card, or its card left out changes of a turn
/// that may have ended meanwhile. That checkpoint may hold outside edits already, the lawyer's
/// among them, which the new record must keep, so the matter's log is read first. A turn that
/// ran beside none never does.
pub(super) fn logs_edits(
    actor: &Actor,
    thread: ThreadId,
    result: &Result<crate::matter_worker::TurnCheckpoint, crate::error::Error>,
) -> bool {
    let current = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.current.as_deref());
    let absorbed = current.is_some_and(|current| !current.overlap.absorbed.is_empty());
    let left_out = result
        .as_ref()
        .is_ok_and(|done| !done.checkpoint.left_out.is_empty());
    absorbed || left_out
}

/// Puts `OutsideEdits` on `checkpoint` in `close`: `files` with those already logged on it (the
/// reducer keeps the last `OutsideEdits` of a checkpoint), one per path, sorted.
fn push_edits(
    actor: &Actor,
    matter: MatterId,
    close: &mut TurnClose,
    checkpoint: CheckpointId,
    files: Vec<FileChange>,
) {
    let stream = StreamId::Matter(matter);
    let mut all: BTreeMap<MatterPath, FileChange> = BTreeMap::new();
    let logged = actor
        .loaded(stream)
        .and_then(|loaded| loaded.state.as_matter())
        .and_then(|state| {
            state.entries().values().rev().find_map(|entry| {
                let ViewEntry::Version(version) = entry else {
                    return None;
                };
                (version.checkpoint == checkpoint).then(|| version.files.clone())
            })
        })
        .unwrap_or_default();
    let in_flight = actor.pipeline.unreleased(stream).filter_map(|envelope| {
        let DomainEvent::OutsideEdits(edits) = &envelope.event else {
            return None;
        };
        (edits.checkpoint == checkpoint).then(|| edits.files.clone())
    });
    let earlier: Vec<Vec<FileChange>> = std::iter::once(logged).chain(in_flight).collect();
    for file in earlier.into_iter().flatten().chain(files) {
        all.insert(file.path().clone(), file);
    }
    let files = all.into_values().collect();
    let edits = OutsideEdits { checkpoint, files };
    close.push_matter(matter, MatterEvent::plain(edits));
}

/// `thread`'s turn ends: each turn running beside it in `matter` keeps the papers its card
/// credits, or, when it has no card (its after-turn checkpoint or that checkpoint's events
/// failed), its tool calls' places: those papers stay on that turn's card, naming it, since
/// they are on no other, unless its end logged them as outside edits and they hold that still.
pub(super) fn ended(actor: &mut Actor, matter: MatterId, thread: ThreadId) {
    let Some(own) = current(actor, thread) else {
        return;
    };
    let card = own.overlap.card.take();
    let places = own.overlap.ending.clone();
    let logs = std::mem::take(&mut own.overlap.logs);
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
            (None, Some(places)) => {
                running.overlap.failed.push((thread, places.snapshot()));
            }
            (None, None) => {}
        }
        // Set only by an end with no card (`log_absorbed`).
        running.overlap.logged.extend(logs.iter().cloned());
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
    let turns: BTreeMap<ThreadId, TurnId> = running
        .iter()
        .filter_map(|(other, _)| Some((*other, current_turn(actor, *other)?)))
        .collect();
    let own = current(actor, thread)?;
    if own.overlap.beside.is_empty() {
        return None;
    }
    own.overlap.measured = turns;
    Some(Box::new(TurnOverlap {
        since: own.checkpoint_before?,
        running,
        failed: own.overlap.failed.clone(),
        ended: own.overlap.ended.clone(),
        logged: own.overlap.logged.clone(),
    }))
}

/// The turns running in `matter` that ran beside `thread`'s, with their tool calls' places, which
/// the matter's thread reads when the job runs.
fn running_beside(
    actor: &Actor,
    matter: MatterId,
    thread: ThreadId,
) -> Vec<(ThreadId, SharedPaths)> {
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
pub(super) fn named_by_others(
    actor: &Actor,
    matter: MatterId,
    thread: ThreadId,
) -> Vec<(ThreadId, SharedPaths)> {
    let Some(entry) = actor.matters.get(&matter) else {
        return Vec::new();
    };
    entry
        .turns
        .running
        .iter()
        .filter(|other| **other != thread)
        .filter_map(|other| Some((*other, actor.threads.get(other)?.turn.current.as_deref()?)))
        .filter(|(_, current)| current.overlap.after.is_none())
        .map(|(other, current)| {
            let paths = current.overlap.ending.as_ref();
            (other, paths.unwrap_or(&current.tool_paths).clone())
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
