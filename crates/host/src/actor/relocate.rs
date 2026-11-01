//! `RelocateMatter` ("Locate folder"): a matter whose folder moved is pointed at the folder the
//! lawyer picked, once that folder's `.clauseo/matter.json` names the matter.
//!
//! Order: the matter must be known and the folder ref registered; the matter's writer must be
//! free (`matter.turn_running`); then, on the matter's worker, the registry check (the folder is
//! not, does not hold and is not inside another registered or claimed matter folder:
//! `matter.folder_refused`) and `Matter::locate`, which refuses what import refuses for the
//! place and a folder whose marker is missing, unreadable or names another matter. Only after
//! `Ok` does one commit carry `MatterRelocated`, the matter's row with the new path and the
//! receipt. The host's row moves when that commit is durable; a failed commit sends the worker
//! back to the recorded folder. A refusal stores nothing, so the matter stays as it was.
//!
//! The relocate holds the matter's writer from its checks until its commit returns or it is
//! refused: a turn sent meanwhile queues behind it, and an undo or another relocate is refused
//! (`matter.turn_running`). A turn whose agent was started in the old folder starts it again in
//! the new one when it takes the writer (step 4). The picked folder is claimed on the actor
//! (`Actor::claims`) until the command is answered, so other locates and relinks count it.

use std::path::PathBuf;

use clauseo_protocol::{
    CommandId, CommandOutcome, DomainEvent, ErrorCode, FolderRefId, MatterId, MatterRelocated,
    StreamId,
};

use super::commands::{AfterCommand, CommandCtx};
use super::input::MatterTag;
use super::matters::{registry, start_worker};
use super::{Actor, Tag, turn};
use crate::error::Error;
use crate::matter_worker::{MatterJob, MatterReply};
use crate::pipeline::Draft;

/// `RelocateMatter`: the state checks, then the locate on the matter's worker.
pub(super) fn relocate_matter(
    actor: &mut Actor,
    ctx: &CommandCtx,
    matter: MatterId,
    folder: FolderRefId,
) {
    if let Err(error) = begin(actor, ctx, matter, folder) {
        actor.finish_command(ctx.id, Err(error));
    }
}

fn begin(
    actor: &mut Actor,
    ctx: &CommandCtx,
    matter: MatterId,
    folder: FolderRefId,
) -> Result<(), Error> {
    actor.check_matter(matter)?;
    let folder = actor.folders.get(&folder).cloned().ok_or_else(|| {
        Error::refused(ErrorCode::CommandInvalid, format!("no folder ref {folder}"))
    })?;
    if turn::writer_busy(actor, matter) {
        return Err(Error::refused(
            ErrorCode::MatterTurnRunning,
            "a turn is running in this matter; wait for it or cancel it",
        ));
    }
    let registry = registry(actor, matter);
    let sent = send_locate(actor, ctx, matter, folder.clone(), registry);
    if sent.is_err() {
        turn::release_relocate(actor, matter);
    } else {
        actor.claims.insert(ctx.id, (matter, folder));
    }
    sent
}

/// Takes the writer and sends the locate to the matter's worker, starting it first.
fn send_locate(
    actor: &mut Actor,
    ctx: &CommandCtx,
    matter: MatterId,
    folder: PathBuf,
    registry: crate::matter_worker::Registry,
) -> Result<(), Error> {
    let started = match actor.check_matter(matter)?.worker {
        Some(_) => None,
        None => Some(start_worker(actor, matter)?),
    };
    let entry = actor
        .matters
        .get_mut(&matter)
        .ok_or_else(|| Error::Internal(format!("matter {matter} vanished")))?;
    if let Some(worker) = started {
        entry.worker = Some(worker);
    }
    if !turn::hold_for_relocate(actor, matter) {
        return Err(Error::Internal(format!(
            "matter {matter}'s writer was taken during its relocate's checks"
        )));
    }
    let entry = actor
        .matters
        .get(&matter)
        .ok_or_else(|| Error::Internal(format!("matter {matter} vanished")))?;
    let worker = entry
        .worker
        .as_ref()
        .ok_or_else(|| Error::Internal(format!("matter {matter} has no worker")))?;
    let job = MatterJob::Locate {
        id: matter,
        folder: folder.clone(),
        current: PathBuf::from(&entry.row.folder_path),
        registry,
    };
    worker.send(
        job,
        MatterTag::Relocate {
            command: ctx.id,
            folder,
        },
    )?;
    actor.work.matter_jobs += 1;
    Ok(())
}

/// The worker answered the locate: commit it, or pass the refusal on.
pub(super) fn located(
    actor: &mut Actor,
    matter: MatterId,
    command: CommandId,
    folder: &std::path::Path,
    reply: MatterReply,
) {
    let result = match reply {
        MatterReply::Located(result) => result.map(|_last| ()),
        MatterReply::Created(_)
        | MatterReply::Imported(_)
        | MatterReply::Opened(_)
        | MatterReply::BeforeTurn(_)
        | MatterReply::AfterTurn(_)
        | MatterReply::Undone(_)
        | MatterReply::Restored(_)
        | MatterReply::Compared(_)
        | MatterReply::Previewed(_)
        | MatterReply::HeldOpen(_)
        | MatterReply::Papers(_) => Err(Error::Internal(
            "a matter's locate got another job's reply".to_owned(),
        )),
    };
    if let Err(error) = result {
        // The worker refused before holding the new folder, or went back to the recorded one.
        turn::release_relocate(actor, matter);
        actor.finish_command(command, Err(error));
        return;
    }
    if let Err(error) = commit(actor, matter, command, folder) {
        // The worker holds the matter at the new folder: put it back on the recorded one
        // before any turn's job reaches it.
        reopen_recorded(actor, matter);
        turn::release_relocate(actor, matter);
        actor.finish_command(command, Err(error));
    }
}

/// One commit: `MatterRelocated`, the row with the new path, the receipt.
fn commit(
    actor: &mut Actor,
    matter: MatterId,
    command: CommandId,
    folder: &std::path::Path,
) -> Result<(), Error> {
    let hash = actor
        .commands
        .get(&command)
        .map(|pending| pending.hash)
        .ok_or_else(|| Error::Internal(format!("command {command} vanished")))?;
    let ctx = CommandCtx { id: command, hash };
    let path = folder.to_string_lossy().into_owned();
    let mut row = actor.check_matter(matter)?.row.clone();
    row.folder_path.clone_from(&path);
    let now = actor.now();
    let stream = StreamId::Matter(matter);
    let outcome = CommandOutcome::Done;
    let mut draft = Draft::new();
    actor.pipeline.push(
        &mut draft,
        stream,
        DomainEvent::MatterRelocated(MatterRelocated {
            folder: path.clone(),
        }),
        Some(command),
        now.wall,
        &mut actor.ids,
    )?;
    draft.matter_row(row);
    draft.receipt(ctx.receipt(Some(stream), outcome, now.wall));
    let tag = Tag::Command {
        id: command,
        outcome,
        after: AfterCommand::Relocated {
            matter,
            folder: path,
        },
    };
    actor.send_commit(draft, tag).map(|_| ())
}

/// The relocate's commit returned: on `Ok` the host's row names the new folder; on a failure
/// the worker goes back to the recorded folder. Then the writer passes to the next queued turn,
/// whose agent, if it was started in the old folder, starts again at step 4.
pub(super) fn committed(actor: &mut Actor, matter: MatterId, folder: String, durable: bool) {
    if durable {
        if let Some(entry) = actor.matters.get_mut(&matter) {
            entry.row.folder_path = folder;
        }
    } else {
        reopen_recorded(actor, matter);
    }
    turn::release_relocate(actor, matter);
}

/// Sends the matter's worker an open of the folder its row records.
///
/// `Matter::locate` has already pointed the history's `core.worktree` at the new folder, and
/// `matters` offers no way to write it back but an open. When the recorded folder is still there
/// that open rewrites it; when it is gone (the usual case after a move) the open fails first and
/// `core.worktree` stays on the new folder while the row names the old one. That leftover is
/// safe: the matter reads `missing`, relink refuses (the folder is not the recorded one), and
/// the next `RelocateMatter` sets both again.
fn reopen_recorded(actor: &mut Actor, matter: MatterId) {
    let Some(entry) = actor.matters.get(&matter) else {
        return;
    };
    let Some(worker) = entry.worker.as_ref() else {
        return;
    };
    let open = MatterJob::Open {
        id: matter,
        folder: PathBuf::from(&entry.row.folder_path),
        relink: None,
    };
    match worker.send(open, MatterTag::Open) {
        Ok(()) => actor.work.matter_jobs += 1,
        Err(error) => tracing::warn!(%matter, %error, "could not reopen a matter's folder"),
    }
}
