//! Matters: `CreateMatter` (a new folder or an import), the Scratch matter on the first start,
//! and each matter's worker thread, started and opened on first use.
//!
//! A matter's worker starts lazily: at start-up the host only lists matters, and the first
//! turn or undo of a matter starts its thread and opens it. Most matters are not touched in a
//! run, and an open reads the matter's history (timeline, stat cache, tree check).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_matters::{DEFAULT_MARKER, NewMatter};
use clauseo_protocol::{
    CheckpointKind, CheckpointRecorded, CommandOutcome, DomainEvent, ErrorCode, ImportSummary,
    MatterCreated, MatterId, MatterKind, MatterSource, StreamId, Title,
};
use clauseo_store::MatterRow;

use super::commands::{AfterCommand, CommandCtx};
use super::input::{Input, MatterTag};
use super::{Actor, MatterEntry, Tag, turn};
use crate::error::Error;
use crate::matter_worker::{ImportEnd, MatterJob, MatterReply, MatterWorker, matter_settings};
use crate::pipeline::{Draft, Failure};

/// The Scratch matter's name.
const SCRATCH: &str = "Scratch";

/// A matter whose create or import runs on its new worker.
pub(crate) struct PendingMatter {
    new: NewMatter,
    name: String,
    kind: MatterKind,
    worker: MatterWorker<MatterTag>,
    /// Set by shutdown to end an import early.
    cancel: Arc<AtomicBool>,
}

/// Starts a worker for matter `matter`, posting its replies to the actor.
fn start_worker(actor: &Actor, matter: MatterId) -> Result<MatterWorker<MatterTag>, Error> {
    let post = actor.post.clone();
    MatterWorker::start(
        matter_settings(actor.effective.data_dir()),
        Box::new(move |tag, reply| {
            let _gone = post.try_send(Input::Matter { matter, tag, reply });
        }),
    )
}

/// The worker of a known matter, started and sent its open on first use.
pub(crate) fn matter_worker(
    actor: &mut Actor,
    matter: MatterId,
) -> Result<&MatterWorker<MatterTag>, Error> {
    let started = match actor.check_matter(matter)?.worker {
        Some(_) => None,
        None => Some(start_worker(actor, matter)?),
    };
    let entry = actor
        .matters
        .get_mut(&matter)
        .ok_or_else(|| Error::Internal(format!("matter {matter} vanished")))?;
    if let Some(worker) = started {
        let folder = PathBuf::from(&entry.row.folder_path);
        worker.send(MatterJob::Open { id: matter, folder }, MatterTag::Open)?;
        actor.work.matter_jobs += 1;
        entry.worker = Some(worker);
    }
    entry
        .worker
        .as_ref()
        .ok_or_else(|| Error::Internal(format!("matter {matter} has no worker")))
}

/// Starts a new matter's worker with its create or import job.
fn begin_matter(
    actor: &mut Actor,
    name: String,
    kind: MatterKind,
    folder: PathBuf,
    import: bool,
    tag: MatterTag,
) -> Result<Box<PendingMatter>, Error> {
    let now = actor.now();
    let new = NewMatter {
        id: actor.ids.mint(now.wall)?,
        folder,
        created_at: now.wall,
        marker: DEFAULT_MARKER,
        checkpoint: actor.ids.mint(now.wall)?,
    };
    let worker = start_worker(actor, new.id)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let job = if import {
        let mut refused = actor.refused_folders.clone();
        refused.push(actor.effective.data_dir().to_path_buf());
        MatterJob::Import {
            new: new.clone(),
            refused,
            cancel: Arc::clone(&cancel),
        }
    } else {
        MatterJob::Create(new.clone())
    };
    worker.send(job, tag)?;
    actor.work.matter_jobs += 1;
    Ok(Box::new(PendingMatter {
        new,
        name,
        kind,
        worker,
        cancel,
    }))
}

/// `CreateMatter`: a new folder `<parent>/<name>` (refused when it exists) or an import of
/// an existing folder, on the new matter's worker; then one commit.
pub(super) fn create_matter(
    actor: &mut Actor,
    ctx: &CommandCtx,
    name: &Title,
    source: MatterSource,
) {
    let begun = matter_folder(actor, name, &source).and_then(|(folder, import)| {
        begin_matter(
            actor,
            name.as_str().to_owned(),
            MatterKind::Folder,
            folder,
            import,
            MatterTag::Create(ctx.id),
        )
    });
    match begun {
        Ok(pending) => {
            if let Some(command) = actor.commands.get_mut(&ctx.id) {
                command.matter = Some(pending);
            }
        }
        Err(error) => actor.finish_command(ctx.id, Err(error)),
    }
}

/// The folder a `CreateMatter` names, and whether it is an import.
fn matter_folder(
    actor: &Actor,
    name: &Title,
    source: &MatterSource,
) -> Result<(PathBuf, bool), Error> {
    let (folder, import) = match source {
        MatterSource::NewFolder { folder } => (folder, false),
        MatterSource::ExistingFolder { folder } => (folder, true),
    };
    let path = actor.folders.get(folder).ok_or_else(|| {
        Error::refused(ErrorCode::CommandInvalid, format!("no folder ref {folder}"))
    })?;
    if import {
        return Ok((path.clone(), true));
    }
    let path = path.join(name.as_str());
    if path.exists() {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            format!("{} already exists; import it instead", path.display()),
        ));
    }
    Ok((path, false))
}

/// Starts creating the Scratch matter in the first free folder of
/// `<default_matters_dir>/Scratch`, `Scratch 2`, ...
pub(super) fn create_scratch(actor: &mut Actor) {
    let base = actor.effective.default_matters_dir().to_path_buf();
    let folder = (1_u32..=u32::MAX)
        .map(|n| match n {
            1 => base.join(SCRATCH),
            _ => base.join(format!("{SCRATCH} {n}")),
        })
        .find(|folder| !folder.exists());
    let Some(folder) = folder else {
        return;
    };
    let begun = begin_matter(
        actor,
        SCRATCH.to_owned(),
        MatterKind::Scratch,
        folder,
        false,
        MatterTag::Scratch,
    );
    match begun {
        Ok(pending) => actor.scratch = Some(pending),
        Err(error) => tracing::warn!(%error, "could not start the Scratch matter"),
    }
}

/// A matter worker answered.
pub(super) fn on_matter_reply(
    actor: &mut Actor,
    matter: MatterId,
    tag: MatterTag,
    reply: MatterReply,
) {
    actor.work.matter_jobs = actor.work.matter_jobs.saturating_sub(1);
    match tag {
        MatterTag::Create(id) => {
            let pending = actor
                .commands
                .get_mut(&id)
                .and_then(|command| command.matter.take());
            let Some(pending) = pending else {
                return;
            };
            let outcome = CommandOutcome::MatterCreated { matter };
            let committed = made(reply).and_then(|import| {
                let (draft, entry) = matter_commit(actor, *pending, import, Some((id, outcome)))?;
                let tag = Tag::Command {
                    id,
                    outcome,
                    after: AfterCommand::Matter(Box::new(entry)),
                };
                actor.send_commit(draft, tag).map(|_| ())
            });
            if let Err(error) = committed {
                actor.finish_command(id, Err(error));
            }
        }
        MatterTag::Scratch => {
            let Some(pending) = actor.scratch.take() else {
                return;
            };
            let committed = made(reply).and_then(|import| {
                let (draft, entry) = matter_commit(actor, *pending, import, None)?;
                actor
                    .send_commit(draft, Tag::Scratch(Box::new(entry)))
                    .map(|_| ())
            });
            if let Err(error) = committed {
                tracing::warn!(%error, "could not create the Scratch matter");
            }
        }
        MatterTag::Open => {
            if let MatterReply::Opened(Err(error)) = reply {
                tracing::warn!(%matter, %error, "could not open a matter; it is tried again");
            }
        }
        MatterTag::Turn(job) => turn::matter_reply(actor, matter, job, reply),
    }
}

/// The commit of a matter just made: `MatterCreated`, `CheckpointRecorded { Import }` and
/// the row (with the receipt for a command), and the entry it adds once durable.
fn matter_commit(
    actor: &mut Actor,
    pending: PendingMatter,
    import: Option<ImportSummary>,
    command: Option<(clauseo_protocol::CommandId, CommandOutcome)>,
) -> Result<(Draft, MatterEntry), Error> {
    let PendingMatter {
        new,
        name,
        kind,
        worker,
        cancel: _,
    } = pending;
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
    actor.pipeline.push(
        &mut draft,
        stream,
        DomainEvent::CheckpointRecorded(checkpoint),
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
    };
    draft.matter_row(row.clone());
    if let Some((id, outcome)) = command {
        let hash = actor
            .commands
            .get(&id)
            .map(|command| command.hash)
            .ok_or_else(|| Error::Internal(format!("command {id} vanished")))?;
        let ctx = CommandCtx { id, hash };
        draft.receipt(ctx.receipt(Some(stream), outcome, now.wall));
    }
    let entry = MatterEntry {
        row,
        worker: Some(worker),
        turns: turn::MatterTurns::default(),
    };
    Ok((draft, entry))
}

/// The Scratch matter's commit returned.
pub(super) fn scratch_committed(
    actor: &mut Actor,
    entry: MatterEntry,
    outcome: Result<(), &Failure>,
) {
    match outcome {
        Ok(()) => {
            actor.matters.insert(entry.row.id, entry);
        }
        Err(failure) => {
            tracing::warn!(error = %failure.error, "the Scratch matter was not stored");
        }
    }
}

/// Asks every import in progress to stop (shutdown).
pub(super) fn cancel_imports(actor: &Actor) {
    for pending in actor.commands.values() {
        if let Some(matter) = &pending.matter {
            matter.cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// The import summary of a create or import that succeeded, `None` for a new folder.
fn made(reply: MatterReply) -> Result<Option<ImportSummary>, Error> {
    match reply {
        MatterReply::Created(result) => result.map(|()| None),
        MatterReply::Imported(result) => match result? {
            ImportEnd::Done { summary, warnings } => {
                for warning in &warnings {
                    tracing::info!(?warning, "import warning");
                }
                Ok(Some(summary))
            }
            ImportEnd::Cancelled => Err(Error::Closed),
        },
        MatterReply::Opened(_)
        | MatterReply::BeforeTurn(_)
        | MatterReply::AfterTurn(_)
        | MatterReply::Undone(_) => Err(Error::Internal(
            "a matter's create got another job's reply".to_owned(),
        )),
    }
}
