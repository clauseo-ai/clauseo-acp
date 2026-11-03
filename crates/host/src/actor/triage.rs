//! The rail's triage (0045): `SnoozeThread`, `MarkThreadDone`, the morning sweep (`SweepDone`)
//! and the snooze timer (`triage/wake.rs`). Each logs `thread_snoozed` or `thread_done` in the
//! thread's stream; the reducer keeps both on the header, and its wake rule clears them.
//!
//! A commit of an unloaded stream is not folded (`streams.rs`), so every thread a triage event
//! goes to is loaded first, and the decision is made on its loaded state: the header the commit
//! then folds into is the one `ListThreads` and the next sweep read.

mod wake;

use std::cell::RefCell;
use std::rc::Rc;

use clauseo_protocol::{
    CommandOutcome, DomainEvent, DoneBy, ErrorCode, HeaderEntry, MAX_DAY_START_SKEW_MS,
    MAX_SNOOZE_AHEAD_MS, SnoozeBy, StreamId, ThreadDone, ThreadId, ThreadSnoozed, ThreadStatus,
    Timestamp,
};

pub(crate) use wake::{Triage, on_wake, tick, wake_committed};

use super::commands::{AfterCommand, CommandCtx};
use super::{Actor, Tag, turn};
use crate::error::Error;
use crate::pipeline::Draft;

/// `SnoozeThread`: an `until` not after now, or more than [`MAX_SNOOZE_AHEAD_MS`] ahead, is
/// `command.invalid`; otherwise `thread_snoozed` (`by: lawyer`) and `Done`.
pub(super) fn snooze(
    actor: &mut Actor,
    ctx: &CommandCtx,
    thread: ThreadId,
    until: Option<Timestamp>,
) {
    if let Some(until) = until {
        let now = actor.now().wall.as_millis();
        let ahead = until.as_millis().saturating_sub(now);
        let why = if ahead <= 0 {
            Some("a snooze ends after now")
        } else if ahead > MAX_SNOOZE_AHEAD_MS {
            Some("a snooze ends at most 366 days ahead")
        } else {
            None
        };
        if let Some(why) = why {
            let refusal = Error::refused(ErrorCode::CommandInvalid, why);
            actor.finish_command(ctx.id, Err(refusal));
            return;
        }
    }
    let event = ThreadSnoozed {
        until,
        by: SnoozeBy::Lawyer,
    };
    log_thread_event(actor, ctx, thread, event.into());
}

/// `MarkThreadDone`: `thread_done` (`by: lawyer`) and `Done`.
pub(super) fn mark_done(actor: &mut Actor, ctx: &CommandCtx, thread: ThreadId, done: bool) {
    let event = ThreadDone {
        done,
        by: DoneBy::Lawyer,
    };
    log_thread_event(actor, ctx, thread, event.into());
}

/// Logs `event` in `thread`'s loaded stream with the command's receipt; archived threads too
/// (no command refuses an archived thread). Every event is logged, a repeat too: the loaded
/// state is only what is durable, so it cannot tell a repeat from an undo of a change still in
/// flight.
fn log_thread_event(actor: &mut Actor, ctx: &CommandCtx, thread: ThreadId, event: DomainEvent) {
    if let Err(error) = super::commands::listed_thread(actor, thread) {
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    let ctx = *ctx;
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let sent = loaded.and_then(|()| {
                let now = actor.now().wall;
                let mut draft = Draft::new();
                // Text the thread holds goes first, so it keeps its place.
                turn::flush_text(actor, thread, &mut draft)?;
                actor.pipeline.push(
                    &mut draft,
                    stream,
                    event,
                    Some(ctx.id),
                    now,
                    &mut actor.ids,
                )?;
                draft.receipt(ctx.receipt(Some(stream), CommandOutcome::Done, now));
                let tag = Tag::Command {
                    id: ctx.id,
                    outcome: CommandOutcome::Done,
                    after: AfterCommand::Nothing,
                };
                actor.send_commit(draft, tag).map(|_| ())
            });
            if let Err(error) = sent {
                actor.finish_command(ctx.id, Err(error));
            }
            actor.release(stream);
        }),
    );
}

/// What the sweep and the wake read of one thread at decision time.
struct Facts<'a> {
    header: &'a HeaderEntry,
    /// A permission or an elicitation of it waits (only known while loaded).
    asking: bool,
    /// Its stream has a commit in flight or is blocked: its header is about to change, or
    /// cannot, so nothing is decided on it now.
    writing: bool,
}

/// The header of `thread` (the loaded state's, else the one kept at its last unload), whether
/// it asks the lawyer for something and whether its stream is being written; `None` for a
/// thread not in a listed matter or one whose header is not known.
fn facts(actor: &Actor, thread: ThreadId) -> Option<Facts<'_>> {
    let entry = actor.threads.get(&thread)?;
    let stream = StreamId::Thread(thread);
    if !actor.matters.contains_key(&entry.row.matter) {
        return None;
    }
    let writing = actor.pipeline.in_flight(stream) || actor.pipeline.blocked(stream).is_some();
    let loaded = actor.loaded(stream);
    let header = loaded
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or(entry.header.as_ref())?;
    let asking = loaded.is_some_and(|loaded| {
        loaded.sent.permissions().next().is_some() || loaded.sent.elicitations().next().is_some()
    });
    Some(Facts {
        header,
        asking,
        writing,
    })
}

/// Whether the morning sweep marks `thread` done: it is quiet (nothing asks the lawyer, no turn
/// runs or waits), its last ending is seen, its newest event is before `day_start`, and it is
/// not archived, done already or snoozed past `now`.
fn sweeps(actor: &Actor, thread: ThreadId, day_start: Timestamp, now: Timestamp) -> bool {
    let (Some(facts), Some(entry)) = (facts(actor, thread), actor.threads.get(&thread)) else {
        return false;
    };
    let header = facts.header;
    let quiet = matches!(
        header.status,
        ThreadStatus::Idle | ThreadStatus::AgentExited | ThreadStatus::IdleStopped
    ) && !facts.asking
        && !header.queue_held
        && !turn::turn_running(actor, thread);
    let before = actor
        .tails
        .get(&StreamId::Thread(thread))
        .is_some_and(|&last| last < day_start);
    quiet
        && before
        && !facts.writing
        && !entry.row.archived
        && !header.done
        && header.snoozed_until.is_none_or(|until| until <= now)
        && super::rail::seen(entry)
}

/// Whether `thread`, in a listed matter, has no header the host knows yet (created and never
/// loaded since the host started): the sweep loads it to find out.
fn unknown(actor: &Actor, thread: ThreadId) -> bool {
    actor.threads.get(&thread).is_some_and(|entry| {
        actor.matters.contains_key(&entry.row.matter)
            && entry.header.is_none()
            && actor.loaded(StreamId::Thread(thread)).is_none()
    })
}

/// `SweepDone`: a `day_start` more than [`MAX_DAY_START_SKEW_MS`] from now is
/// `command.invalid`. Otherwise every thread [`sweeps`] picks from what the host holds, and
/// every one whose header it does not know yet, is loaded, picked again on its loaded state,
/// and marked done (`by: morning_sweep`), all in one commit with the receipt; `Done`.
pub(super) fn sweep_done(actor: &mut Actor, ctx: &CommandCtx, day_start: Timestamp) {
    let now = actor.now().wall;
    let skew = day_start.as_millis().saturating_sub(now.as_millis());
    if skew.unsigned_abs() > MAX_DAY_START_SKEW_MS.unsigned_abs() {
        let refusal = Error::refused(
            ErrorCode::CommandInvalid,
            "day_start is more than 26 hours from now",
        );
        actor.finish_command(ctx.id, Err(refusal));
        return;
    }
    let picked: Vec<ThreadId> = actor
        .threads
        .keys()
        .copied()
        .filter(|&thread| sweeps(actor, thread, day_start, now) || unknown(actor, thread))
        .collect();
    let ctx = *ctx;
    load_all(
        actor,
        &picked,
        Box::new(move |actor: &mut Actor, loaded: &[ThreadId]| {
            if let Err(error) = commit_sweep(actor, &ctx, day_start, loaded) {
                actor.finish_command(ctx.id, Err(error));
            }
        }),
    );
}

/// The sweep's one commit: `thread_done` for each of `loaded` still picked now, and the receipt.
fn commit_sweep(
    actor: &mut Actor,
    ctx: &CommandCtx,
    day_start: Timestamp,
    loaded: &[ThreadId],
) -> Result<(), Error> {
    let now = actor.now().wall;
    let mut draft = Draft::new();
    for &thread in loaded {
        if !sweeps(actor, thread, day_start, now) {
            continue;
        }
        let event = ThreadDone {
            done: true,
            by: DoneBy::MorningSweep,
        };
        let stream = StreamId::Thread(thread);
        actor.pipeline.push(
            &mut draft,
            stream,
            event.into(),
            Some(ctx.id),
            now,
            &mut actor.ids,
        )?;
    }
    draft.receipt(ctx.receipt(None, CommandOutcome::Done, now));
    let tag = Tag::Command {
        id: ctx.id,
        outcome: CommandOutcome::Done,
        after: AfterCommand::Nothing,
    };
    actor.send_commit(draft, tag).map(|_| ())
}

/// What runs once every thread of a [`load_all`] is loaded or failed to: given those loaded.
type AllLoaded = Box<dyn FnOnce(&mut Actor, &[ThreadId])>;

/// Whether a [`load_all`] holds `thread`'s stream loaded: `streams::release` keeps it.
pub(crate) fn holds(actor: &Actor, thread: ThreadId) -> bool {
    actor.triage.held.contains_key(&thread)
}

/// Loads every one of `threads` and holds each loaded until all are, then runs `then` with
/// those that loaded, then lets them all go (a stream with a commit `then` sent stays loaded
/// until it returns). A thread that fails to load is left out. With no thread, `then` runs at
/// once.
fn load_all(actor: &mut Actor, threads: &[ThreadId], then: AllLoaded) {
    struct Join {
        left: usize,
        loaded: Vec<ThreadId>,
        then: Option<AllLoaded>,
    }
    if threads.is_empty() {
        then(actor, &[]);
        return;
    }
    for &thread in threads {
        *actor.triage.held.entry(thread).or_default() += 1;
    }
    let join = Rc::new(RefCell::new(Join {
        left: threads.len(),
        loaded: Vec::with_capacity(threads.len()),
        then: Some(then),
    }));
    for &thread in threads {
        let join = Rc::clone(&join);
        let all = threads.to_vec();
        actor.with_loaded(
            StreamId::Thread(thread),
            Box::new(move |actor: &mut Actor, result: Result<(), Error>| {
                let ready = {
                    let mut join = join.borrow_mut();
                    match result {
                        Ok(()) => join.loaded.push(thread),
                        Err(error) => tracing::warn!(%thread, %error, "triage: no load"),
                    }
                    join.left = join.left.saturating_sub(1);
                    if join.left == 0 {
                        join.then
                            .take()
                            .map(|then| (then, std::mem::take(&mut join.loaded)))
                    } else {
                        None
                    }
                };
                if let Some((then, loaded)) = ready {
                    then(actor, &loaded);
                    for thread in all {
                        let held = &mut actor.triage.held;
                        if let Some(count) = held.get_mut(&thread) {
                            *count = count.saturating_sub(1);
                            if *count == 0 {
                                held.remove(&thread);
                            }
                        }
                        actor.release(StreamId::Thread(thread));
                    }
                }
            }),
        );
    }
}
