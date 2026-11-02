//! `MarkSeen`: the lawyer has seen a thread up to now. The thread's row keeps the stream's last
//! `seq` as its seen mark (`ThreadRow::seen_seq`, store schema 3), written with the receipt in one
//! commit and no event. A thread is seen while no turn ended after the mark (`rail::seen`); the
//! matter's `ready` and `stopped` count only threads not seen.
//!
//! The mark is the last `seq` sent, not only the durable one: a `TurnEnded` the window already
//! shows (its patches go out before it is durable) is seen with the rest. Like the other row
//! commands, the host's row changes when the commit is sent and goes back if it fails.

use clauseo_protocol::{CommandOutcome, StreamId, ThreadId};

use super::commands::{AfterCommand, CommandCtx};
use super::{Actor, Tag};
use crate::error::Error;
use crate::pipeline::Draft;

/// `MarkSeen`: refused for an unknown thread or one of a forgotten matter
/// (`command.unknown_thread`); otherwise the row with its new mark and the receipt, `Done`.
pub(super) fn mark_seen(actor: &mut Actor, ctx: &CommandCtx, thread: ThreadId) {
    if let Err(error) = mark(actor, ctx, thread) {
        actor.finish_command(ctx.id, Err(error));
    }
}

fn mark(actor: &mut Actor, ctx: &CommandCtx, thread: ThreadId) -> Result<(), Error> {
    let before = super::commands::listed_thread(actor, thread)?.row.clone();
    let stream = StreamId::Thread(thread);
    let mut row = before.clone();
    row.seen_seq = actor.pipeline.pending_head(stream);
    let now = actor.now().wall;
    let mut draft = Draft::new();
    draft.thread_row(row.clone());
    draft.receipt(ctx.receipt(Some(stream), CommandOutcome::Done, now));
    if let Some(entry) = actor.threads.get_mut(&thread) {
        entry.row = row;
    }
    let tag = Tag::Command {
        id: ctx.id,
        outcome: CommandOutcome::Done,
        after: AfterCommand::Row {
            before: Box::new(before.clone()),
        },
    };
    let sent = actor.send_commit(draft, tag).map(|_| ());
    if sent.is_err()
        && let Some(entry) = actor.threads.get_mut(&thread)
    {
        entry.row = before;
    }
    sent
}
