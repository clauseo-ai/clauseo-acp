//! The matter commands of the rail: `UpdateMatter` (client, next date, pinned), `OpenMatter`
//! (`last_opened`) and `ForgetMatter`, and bringing a forgotten matter back when its folder is
//! imported again.
//!
//! Each writes the matter's row and the command's receipt in one commit and no event: the
//! details live on the row (store schema 2), `ListMatters` reads rows, and the reducer's matter
//! view does not show them. The host's row changes when the commit is sent and goes back if it
//! fails, so a command that comes meanwhile sees the change (`ForgetMatter` above all: no turn
//! starts in a matter on its way off the list).
//!
//! Forgetting deletes nothing: the row stays (with `forgotten` set), and so do the threads, the
//! log, the checkpoints, the history under the matters root and the folder with its
//! `.clauseo/matter.json`. The matter's entry moves to `Actor::forgotten` without its worker,
//! where no command, query or start-up repair reaches it. Importing the folder again finds the
//! marker naming the forgotten matter and brings it back, threads and versions included.

use std::path::Path;

use clauseo_protocol::{
    ClientName, CommandOutcome, Date, ErrorCode, FieldChange, MatterId, MatterKind, StreamId,
};
use clauseo_store::MatterRow;

use super::commands::{AfterCommand, CommandCtx};
use super::{Actor, Tag, turn};
use crate::error::Error;
use crate::pipeline::Draft;

/// What `UpdateMatter` asks.
pub(super) struct Update {
    pub(super) client: FieldChange<ClientName>,
    pub(super) next_date: FieldChange<Date>,
    pub(super) pinned: Option<bool>,
}

/// `UpdateMatter`: the changed row and the receipt.
pub(super) fn update_matter(actor: &mut Actor, ctx: &CommandCtx, matter: MatterId, update: Update) {
    let done = actor
        .check_matter(matter)
        .map(|entry| entry.row.clone())
        .and_then(|before| {
            let mut row = before.clone();
            apply(&mut row.client, update.client);
            apply(&mut row.next_date, update.next_date);
            if let Some(pinned) = update.pinned {
                row.pinned = pinned;
            }
            commit_row(actor, ctx, before, row)
        });
    if let Err(error) = done {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// `OpenMatter`: `last_opened` becomes now.
pub(super) fn open_matter(actor: &mut Actor, ctx: &CommandCtx, matter: MatterId) {
    let now = actor.now().wall;
    let done = actor
        .check_matter(matter)
        .map(|entry| entry.row.clone())
        .and_then(|before| {
            let row = MatterRow {
                last_opened: Some(now),
                ..before.clone()
            };
            commit_row(actor, ctx, before, row)
        });
    if let Err(error) = done {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// `ForgetMatter`: refused for the Scratch matter (`command.invalid`) and while anything runs
/// in the matter (`matter.turn_running`); otherwise the row with `forgotten` set and the
/// receipt, the entry moved to `Actor::forgotten` at once, and its agents stopped once durable.
pub(super) fn forget_matter(actor: &mut Actor, ctx: &CommandCtx, matter: MatterId) {
    if let Err(error) = forget(actor, ctx, matter) {
        actor.finish_command(ctx.id, Err(error));
    }
}

fn forget(actor: &mut Actor, ctx: &CommandCtx, matter: MatterId) -> Result<(), Error> {
    let before = actor.check_matter(matter)?.row.clone();
    if before.kind == MatterKind::Scratch {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            "the Scratch matter stays on the list",
        ));
    }
    if turn::matter_busy(actor, matter) {
        return Err(Error::refused(
            ErrorCode::MatterTurnRunning,
            "a turn, undo, restore or relocate is running in this matter; wait for it or cancel it",
        ));
    }
    let now = actor.now().wall;
    let row = MatterRow {
        forgotten: Some(now),
        ..before
    };
    let draft = row_draft(ctx, row.clone(), now);
    let mut entry = actor
        .matters
        .remove(&matter)
        .ok_or_else(|| Error::Internal(format!("matter {matter} vanished")))?;
    entry.row = row;
    // The worker goes: the matter may come back from another folder, and its worker would still
    // hold the old one. It is idle (nothing runs in the matter), so the join is short.
    entry.worker = None;
    actor.forgotten.insert(matter, entry);
    let tag = Tag::Command {
        id: ctx.id,
        outcome: CommandOutcome::Done,
        after: AfterCommand::Forget(matter),
    };
    let sent = actor.send_commit(draft, tag).map(|_| ());
    if sent.is_err() {
        forget_committed(actor, matter, false);
    }
    sent
}

/// `ForgetMatter`'s commit returned: stop the matter's agents, or put the matter back on the
/// list with its row as it was.
pub(super) fn forget_committed(actor: &mut Actor, matter: MatterId, durable: bool) {
    if durable {
        turn::stop_agents(actor, matter);
        return;
    }
    if let Some(mut entry) = actor.forgotten.remove(&matter) {
        entry.row.forgotten = None;
        actor.matters.insert(matter, entry);
    }
}

/// Whether `matter` is forgotten: an import that finds its marker brings it back.
pub(super) fn is_forgotten(actor: &Actor, matter: MatterId) -> bool {
    actor.forgotten.contains_key(&matter)
}

/// Brings the forgotten `matter` back, its folder now `folder` (the one whose marker names it)
/// and with the create's details where given; one commit with the row and the receipt answering
/// `MatterCreated { matter }`. It stays forgotten until that commit is durable.
pub(super) fn unforget(
    actor: &mut Actor,
    ctx: &CommandCtx,
    matter: MatterId,
    folder: &Path,
    (client, next_date): (Option<ClientName>, Option<Date>),
) -> Result<(), Error> {
    let before = actor
        .forgotten
        .get(&matter)
        .map(|entry| entry.row.clone())
        .ok_or_else(|| Error::Internal(format!("forgotten matter {matter} vanished")))?;
    let row = MatterRow {
        folder_path: folder.to_string_lossy().into_owned(),
        forgotten: None,
        client: client.or(before.client.clone()),
        next_date: next_date.or(before.next_date),
        ..before
    };
    let now = actor.now().wall;
    let outcome = CommandOutcome::MatterCreated { matter };
    let mut draft = Draft::new();
    draft.matter_row(row.clone());
    draft.receipt(ctx.receipt(Some(StreamId::Matter(matter)), outcome, now));
    let tag = Tag::Command {
        id: ctx.id,
        outcome,
        after: AfterCommand::Unforget(Box::new(row)),
    };
    actor.send_commit(draft, tag).map(|_| ())
}

/// The bring-back's commit returned: once durable the matter is on the list again with `row`;
/// otherwise it stays forgotten as it was.
pub(super) fn unforget_committed(actor: &mut Actor, row: MatterRow, durable: bool) {
    if durable && let Some(mut entry) = actor.forgotten.remove(&row.id) {
        entry.row = row;
        // A new worker opens the folder where it is now, on first use.
        entry.worker = None;
        actor.matters.insert(entry.row.id, entry);
    }
}

/// A row change's commit failed: the host's row goes back to `before`.
pub(super) fn row_failed(actor: &mut Actor, before: MatterRow) {
    if let Some(entry) = actor.matters.get_mut(&before.id) {
        entry.row = before;
    }
}

/// Sends `row` and the receipt, with the host's row changed now and put back if the commit
/// fails.
fn commit_row(
    actor: &mut Actor,
    ctx: &CommandCtx,
    before: MatterRow,
    row: MatterRow,
) -> Result<(), Error> {
    let now = actor.now().wall;
    let draft = row_draft(ctx, row.clone(), now);
    if let Some(entry) = actor.matters.get_mut(&row.id) {
        entry.row = row;
    }
    let tag = Tag::Command {
        id: ctx.id,
        outcome: CommandOutcome::Done,
        after: AfterCommand::MatterRow {
            before: Box::new(before.clone()),
        },
    };
    let sent = actor.send_commit(draft, tag).map(|_| ());
    if sent.is_err() {
        row_failed(actor, before);
    }
    sent
}

/// A commit of `row` and the command's `Done` receipt.
fn row_draft(ctx: &CommandCtx, row: MatterRow, now: clauseo_protocol::Timestamp) -> Draft {
    let stream = StreamId::Matter(row.id);
    let mut draft = Draft::new();
    draft.matter_row(row);
    draft.receipt(ctx.receipt(Some(stream), CommandOutcome::Done, now));
    draft
}

/// Applies an update's change to one detail.
fn apply<T>(detail: &mut Option<T>, change: FieldChange<T>) {
    match change {
        FieldChange::Keep => {}
        FieldChange::Clear => *detail = None,
        FieldChange::Set(value) => *detail = Some(value),
    }
}
