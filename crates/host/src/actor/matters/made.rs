//! A matter made on disk whose record failed. A create or import writes the folder's
//! `.clauseo/matter.json` and the matter's history on its worker, then the actor commits the
//! matter's events and row; when building or storing that commit fails, the folder would keep a
//! marker naming a matter Clauseo has no row for, and every later "New matter" there would be
//! refused as already a matter. So the worker discards what the create or import made
//! (`Matter::discard`: the history, `matter.json`, and `.clauseo` or the folder only when it
//! made them) and only then is the command answered, with the commit's own error, so a retry
//! finds the folder as it was.
//!
//! The logs here name the matter, the error's code and the stage, never a folder: a path names a
//! client.

use clauseo_matters::CommitId;
use clauseo_protocol::{
    CheckpointKind, CheckpointRecorded, CommandId, CommandOutcome, DomainEvent, ImportSummary,
    MatterCreated, MatterId, StreamId,
};
use clauseo_store::MatterRow;

use super::super::input::MatterTag;
use super::super::{Actor, MatterEntry, turn};
use super::{PendingMatter, command_ctx};
use crate::error::Error;
use crate::matter_worker::{MatterJob, MatterReply, MatterWorker, matter_settings};
use crate::pipeline::{Draft, MatterEvent};

/// A made matter's worker, until its commit returns; with the failure to answer, once a discard
/// was sent.
pub(crate) struct Made {
    worker: MatterWorker<MatterTag>,
    /// The command to answer (none for the Scratch matter) and its error.
    failed: Option<(Option<CommandId>, Error)>,
}

impl Made {
    pub(crate) const fn new(worker: MatterWorker<MatterTag>) -> Self {
        Self {
            worker,
            failed: None,
        }
    }
}

/// The commit of a made matter returned: it joins the list with its worker.
pub(crate) fn recorded(actor: &mut Actor, mut entry: MatterEntry) {
    entry.worker = actor.made.remove(&entry.row.id).map(|made| made.worker);
    actor.matters.insert(entry.row.id, entry);
}

/// The made matter `matter` was not recorded (`stage`: `commit` when its commit could not be
/// built or sent, `store` when the store refused it): its worker discards it, and command
/// `command` is answered with `error` when that returns ([`discarded`]).
pub(crate) fn discard(
    actor: &mut Actor,
    command: Option<CommandId>,
    matter: MatterId,
    error: Error,
    stage: &'static str,
) {
    tracing::warn!(%matter, code = %error.code(), stage, "a matter made on disk was not recorded");
    let Some(made) = actor.made.get_mut(&matter) else {
        answer(actor, command, error);
        return;
    };
    match made.worker.send(MatterJob::Discard, MatterTag::Discard) {
        Ok(()) => {
            made.failed = Some((command, error));
            actor.work.matter_jobs += 1;
        }
        Err(closed) => {
            tracing::warn!(%matter, code = %closed.code(), stage = "discard", "not discarded");
            actor.made.remove(&matter);
            answer(actor, command, error);
        }
    }
}

/// The discard of `matter` returned: logged when it failed, then the command is answered with
/// the commit's error.
pub(crate) fn discarded(actor: &mut Actor, matter: MatterId, reply: &MatterReply) {
    if let MatterReply::Discarded(Err(error)) = reply {
        tracing::warn!(%matter, code = %error.code(), stage = "discard", "not discarded");
    } else if !matches!(reply, MatterReply::Discarded(Ok(()))) {
        tracing::error!(?reply, "a discard answered with another job's reply");
    }
    let Some(made) = actor.made.remove(&matter) else {
        return;
    };
    // Joins the worker, idle now that its last job has answered.
    drop(made.worker);
    if let Some((command, error)) = made.failed {
        answer(actor, command, error);
    }
}

fn answer(actor: &mut Actor, command: Option<CommandId>, error: Error) {
    if let Some(command) = command {
        actor.finish_command(command, Err(error));
    }
}

/// Logs a `CreateMatter` that made nothing, at warn: the id minted for it (none when refused
/// before one was), the matter the refusal names (the one already holding the folder), the
/// error's code and the stage (`check` before the worker, `worker` on it).
pub(super) fn not_created(minted: Option<MatterId>, error: &Error, stage: &'static str) {
    let named = if let Error::Refused(refusal) = error {
        refusal.matter()
    } else {
        None
    };
    tracing::warn!(matter = ?minted, ?named, code = %error.code(), stage, "no matter was created");
}

/// The commit of a matter just made: `MatterCreated`, `CheckpointRecorded { Import }`, the
/// matter's row and the root checkpoint's row (with the receipt for a command), and the entry
/// it adds once durable.
pub(super) fn matter_commit(
    actor: &mut Actor,
    pending: PendingMatter,
    (commit, import): (CommitId, Option<ImportSummary>),
    command: Option<(clauseo_protocol::CommandId, CommandOutcome)>,
) -> Result<(Draft, MatterEntry), Error> {
    let PendingMatter {
        new,
        name,
        kind,
        details: (client, next_date),
        worker,
        cancel: _,
    } = pending;
    // Kept aside until the commit returns, so a failure from here on can discard the matter.
    actor.made.insert(new.id, Made::new(worker));
    let now = actor.now();
    let stream = StreamId::Matter(new.id);
    let command_id = command.map(|(id, _)| id);
    let created = MatterCreated::new(name.clone(), kind, import)?;
    let checkpoint = CheckpointRecorded::new(new.checkpoint, CheckpointKind::Import, None, None)?;
    let mut draft = Draft::new();
    let ids = &mut actor.ids;
    actor.pipeline.push(
        &mut draft,
        stream,
        DomainEvent::MatterCreated(created),
        command_id,
        now.wall,
        ids,
    )?;
    actor.pipeline.push_matter(
        &mut draft,
        new.id,
        MatterEvent::checkpoint(checkpoint, commit),
        command_id,
        now.wall,
        ids,
    )?;
    let shadow = matter_settings(actor.effective.data_dir())
        .matters_root
        .join(new.id.to_string());
    let row = MatterRow {
        id: new.id,
        name,
        kind,
        folder_path: new.folder.to_string_lossy().into_owned(),
        folder_locator: None,
        shadow_repo_path: shadow.to_string_lossy().into_owned(),
        created: new.created_at,
        client,
        next_date,
        pinned: false,
        last_opened: None,
        forgotten: None,
    };
    draft.matter_row(row.clone());
    if let Some((id, outcome)) = command {
        let ctx = command_ctx(actor, id)?;
        draft.receipt(ctx.receipt(Some(stream), outcome, now.wall));
    }
    let entry = MatterEntry {
        row,
        worker: None,
        turns: turn::MatterTurns::default(),
    };
    Ok((draft, entry))
}
