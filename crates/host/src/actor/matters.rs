//! Matters: `CreateMatter` (a new folder, picked or in the default location, or an import), the
//! Scratch matter on the first start, and each matter's worker thread, started and opened on
//! first use (relinking a deleted `matter.json` once, when no other matter claims the folder). A
//! refused folder gets one code per reason (`refusal`), and an import of a forgotten matter's
//! folder brings that matter back (`matter_details.rs`). `RelocateMatter` is in `relocate.rs`.
//!
//! A matter's worker starts lazily: at start-up the host only lists matters, and the first
//! turn or undo of a matter starts its thread and opens it. Most matters are not touched in a
//! run, and an open reads the matter's history (timeline, stat cache, tree check).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_matters::{CommitId, DEFAULT_MARKER, NewMatter, Refusal};
use clauseo_protocol::{
    CheckpointKind, CheckpointRecorded, ClientName, CommandOutcome, Date, DomainEvent, ErrorCode,
    HostError, ImportSummary, MatterCreated, MatterId, MatterKind, MatterSource, QueryResult,
    RestorePreview, StreamId, Title,
};
use clauseo_store::MatterRow;

use super::commands::{AfterCommand, CommandCtx};
use super::input::{Input, MatterTag};
use super::{Actor, MatterEntry, Tag, turn};
use crate::error::Error;
use crate::matter_worker::{
    Claim, Claimed, ImportEnd, MatterJob, MatterReply, MatterWorker, Registry, Relink,
    check_registry, matter_settings,
};
use crate::pipeline::{Draft, Failure, MatterEvent};

/// The Scratch matter's name.
const SCRATCH: &str = "Scratch";

/// A matter whose create or import runs on its new worker.
pub(crate) struct PendingMatter {
    new: NewMatter,
    name: String,
    kind: MatterKind,
    /// The create's client and next date.
    details: Details,
    worker: MatterWorker<MatterTag>,
    /// Set by shutdown to end an import early.
    cancel: Arc<AtomicBool>,
}

/// A create's client and next date.
pub(super) type Details = (Option<ClientName>, Option<Date>);

/// Starts a worker for matter `matter`, posting its replies to the actor.
pub(super) fn start_worker(
    actor: &Actor,
    matter: MatterId,
) -> Result<MatterWorker<MatterTag>, Error> {
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
        None => Some((start_worker(actor, matter)?, registry(actor, matter))),
    };
    let entry = actor
        .matters
        .get_mut(&matter)
        .ok_or_else(|| Error::Internal(format!("matter {matter} vanished")))?;
    if let Some((worker, registry)) = started {
        let folder = PathBuf::from(&entry.row.folder_path);
        let relink = Relink {
            created_at: entry.row.created,
            registry,
        };
        let open = MatterJob::Open {
            id: matter,
            folder,
            relink: Some(Box::new(relink)),
        };
        worker.send(open, MatterTag::Open)?;
        actor.work.matter_jobs += 1;
        entry.worker = Some(worker);
    }
    entry
        .worker
        .as_ref()
        .ok_or_else(|| Error::Internal(format!("matter {matter} has no worker")))
}

/// What a folder for `matter` is checked against: the folders import refuses, every other
/// registered matter's folder (forgotten ones too: their folders bring them back), and the
/// folders other commands are making matters of.
pub(super) fn registry(actor: &Actor, matter: MatterId) -> Registry {
    let rows = actor
        .matters
        .values()
        .chain(actor.forgotten.values())
        .map(|entry| (entry.row.id, PathBuf::from(&entry.row.folder_path)));
    let claimed = actor.claims.values().cloned();
    Registry {
        refused: refused_folders(actor),
        others: rows
            .chain(claimed)
            .filter(|(id, _)| *id != matter)
            .collect(),
    }
}

/// The folders import refuses: the configured ones and the data folder.
fn refused_folders(actor: &Actor) -> Vec<PathBuf> {
    let mut refused = actor.refused_folders.clone();
    refused.push(actor.effective.data_dir().to_path_buf());
    refused
}

/// Starts a new matter's worker with its create or import job.
fn begin_matter(
    actor: &mut Actor,
    (name, kind, details): (String, MatterKind, Details),
    folder: PathBuf,
    import: Option<Vec<Claimed>>,
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
    let job = if let Some(registry) = import {
        MatterJob::Import {
            new: new.clone(),
            refused: refused_folders(actor),
            registry,
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
        details,
        worker,
        cancel,
    }))
}

/// `CreateMatter`: a new folder `<parent>/<name>` or `<default location>/<name>` (refused
/// when it exists) or an import of an existing folder, on the new matter's worker; then one
/// commit. A folder that is, holds or sits inside another matter's folder (listed, forgotten or
/// being made; `registry`) is refused with `matter.folder_is_matter`, naming the matter when it
/// is listed: for an import on its worker, after the place check, so a drive root or the home
/// folder is named for what it is even when a matter sits below it; for a new folder here,
/// since only its parent exists.
pub(super) fn create_matter(
    actor: &mut Actor,
    ctx: &CommandCtx,
    name: &Title,
    source: MatterSource,
    details: Details,
) {
    let begun = matter_folder(actor, name, &source)
        .and_then(|(folder, import)| {
            let registry = claimed_folders(actor);
            if import {
                Ok((folder, Some(registry)))
            } else {
                check_registry(&folder, &registry, false).map(|()| (folder, None))
            }
        })
        .and_then(|(folder, import)| {
            begin_matter(
                actor,
                (name.as_str().to_owned(), MatterKind::Folder, details),
                folder,
                import,
                MatterTag::Create(ctx.id),
            )
        });
    match begun {
        Ok(pending) => {
            let claim = (pending.new.id, pending.new.folder.clone());
            actor.claims.insert(ctx.id, claim);
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
    let (parent, import) = match source {
        MatterSource::NewFolder { folder } => (registered(actor, *folder)?, false),
        MatterSource::ExistingFolder { folder } => (registered(actor, *folder)?, true),
        MatterSource::DefaultLocation => {
            (actor.effective.default_matters_dir().to_path_buf(), false)
        }
    };
    if import {
        return Ok((parent, true));
    }
    // The parent as the file system names it, so the registry check compares it with
    // canonical folders; the new folder itself does not exist yet. A parent that does not
    // exist (the default location before its first matter) is taken as written.
    let parent = dunce::canonicalize(&parent).unwrap_or(parent);
    let path = parent.join(name.as_str());
    if path.exists() {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            format!("{} already exists; import it instead", path.display()),
        ));
    }
    Ok((path, false))
}

/// The folder `folder` names, or `command.invalid` when it names none.
fn registered(actor: &Actor, folder: clauseo_protocol::FolderRefId) -> Result<PathBuf, Error> {
    actor
        .folders
        .get(&folder)
        .cloned()
        .ok_or_else(|| Error::refused(ErrorCode::CommandInvalid, format!("no folder ref {folder}")))
}

/// The folders a new matter's folder must not be, hold or sit inside: every listed matter's,
/// every one being made or relocated to, and every forgotten matter's (whose own folder still
/// brings it back).
fn claimed_folders(actor: &Actor) -> Vec<Claimed> {
    let claimed = |claim| {
        move |entry: &MatterEntry| Claimed {
            matter: entry.row.id,
            folder: PathBuf::from(&entry.row.folder_path),
            claim,
        }
    };
    let listed = actor.matters.values().map(claimed(Claim::Listed));
    let forgotten = actor.forgotten.values().map(claimed(Claim::Forgotten));
    let making = actor
        .commands
        .values()
        .filter_map(|command| command.matter.as_deref())
        .map(|pending| Claimed {
            matter: pending.new.id,
            folder: pending.new.folder.clone(),
            claim: Claim::Making,
        });
    // A folder a relocate in flight is moving a matter to counts as one being made. `actor.claims`
    // also holds the creates, so they are listed twice: harmless.
    let relocating = actor.claims.values().map(|(matter, folder)| Claimed {
        matter: *matter,
        folder: folder.clone(),
        claim: Claim::Making,
    });
    listed
        .chain(making)
        .chain(relocating)
        .chain(forgotten)
        .collect()
}

/// A create's or import's failure as the lawyer's form needs it: a refused folder gets the
/// code of its reason, and a folder the host may not read `matter.folder_unreadable`. The home
/// folder and the data folder are told apart from the other refused folders by the entry the
/// refusal names. Any other failure is passed on as it is.
fn refusal(actor: &Actor, error: Error, folder: &Path) -> Error {
    let Error::Matters(failure) = &error else {
        return error;
    };
    if let clauseo_matters::Error::Io { path, source, .. } = failure
        && source.kind() == io::ErrorKind::PermissionDenied
        && path.starts_with(folder)
    {
        return Error::refused(ErrorCode::MatterFolderUnreadable, error.to_string());
    }
    let clauseo_matters::Error::FolderRefused { reason, .. } = failure else {
        return error;
    };
    let code = match reason {
        Refusal::DriveRoot => ErrorCode::MatterFolderIsDrive,
        Refusal::Listed(entry) if actor.home.as_deref() == Some(entry.as_path()) => {
            ErrorCode::MatterFolderIsHome
        }
        Refusal::Listed(entry) if entry.as_path() == actor.effective.data_dir() => {
            ErrorCode::MatterFolderInMattersRoot
        }
        Refusal::Listed(_) => ErrorCode::MatterFolderIsSystem,
        // Named only when on the list: a marker from another data folder or computer names a
        // matter this install has never had, with nothing to open. (A forgotten matter's marker
        // never gets here: the import brings that matter back.)
        Refusal::AlreadyMatter(matter) => {
            let refusal = HostError::new(ErrorCode::MatterFolderIsMatter, error.to_string());
            return Error::Refused(if actor.matters.contains_key(matter) {
                refusal.with_matter(*matter)
            } else {
                refusal
            });
        }
        Refusal::UnreadableMatterJson => ErrorCode::MatterFolderUnreadable,
        Refusal::MattersRoot => ErrorCode::MatterFolderInMattersRoot,
    };
    Error::refused(code, error.to_string())
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
        (SCRATCH.to_owned(), MatterKind::Scratch, (None, None)),
        folder,
        None,
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
            let folder = pending.new.folder.clone();
            let committed = match made(reply) {
                Err(Error::Matters(clauseo_matters::Error::FolderRefused {
                    reason: Refusal::AlreadyMatter(known),
                    ..
                })) if super::matter_details::is_forgotten(actor, known) => {
                    let details = pending.details.clone();
                    command_ctx(actor, id).and_then(|ctx| {
                        super::matter_details::unforget(actor, &ctx, known, &folder, details)
                    })
                }
                made => made.and_then(|import| {
                    let (draft, entry) =
                        matter_commit(actor, *pending, import, Some((id, outcome)))?;
                    let tag = Tag::Command {
                        id,
                        outcome,
                        after: AfterCommand::Matter(Box::new(entry)),
                    };
                    actor.send_commit(draft, tag).map(|_| ())
                }),
            };
            if let Err(error) = committed {
                let error = refusal(actor, error, &folder);
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
        MatterTag::Relocate { command, folder } => {
            super::relocate::located(actor, matter, command, &folder, reply);
        }
        MatterTag::Turn(job) => turn::matter_reply(actor, matter, job, reply),
        MatterTag::Restore(ask) => {
            if let MatterReply::Restored(result) = reply {
                turn::on_restored(actor, &ask, result.map(|done| *done));
            } else {
                tracing::error!(?reply, "a restore answered with another job's reply");
            }
        }
        MatterTag::Query(answer) => {
            let result = match reply {
                MatterReply::Compared(result) => result.map(QueryResult::FileChanges),
                MatterReply::Previewed(result) => result.map(|plan| {
                    let plan = *plan;
                    QueryResult::RestorePreview(RestorePreview {
                        changes: plan.writes,
                        added_since: plan.added_since,
                        kept: plan.kept,
                        locked: plan.locked,
                    })
                }),
                other @ (MatterReply::Created(_)
                | MatterReply::Imported(_)
                | MatterReply::Opened(_)
                | MatterReply::BeforeTurn(_)
                | MatterReply::AfterTurn(_)
                | MatterReply::Undone(_)
                | MatterReply::Restored(_)
                | MatterReply::Located(_)
                | MatterReply::HeldOpen(_)
                | MatterReply::Papers(_)) => Err(Error::Internal(format!(
                    "a query answered with another job's reply: {other:?}"
                ))),
            };
            let _gone = answer.send(result);
        }
        MatterTag::Papers(answer) => super::papers::answer(actor, matter, reply, answer),
    }
}

/// The commit of a matter just made: `MatterCreated`, `CheckpointRecorded { Import }`, the
/// matter's row and the root checkpoint's row (with the receipt for a command), and the entry
/// it adds once durable.
fn matter_commit(
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
        worker: Some(worker),
        turns: turn::MatterTurns::default(),
    };
    Ok((draft, entry))
}

/// The receipt context of the pending command `id`.
fn command_ctx(actor: &Actor, id: clauseo_protocol::CommandId) -> Result<CommandCtx, Error> {
    let hash = actor
        .commands
        .get(&id)
        .map(|command| command.hash)
        .ok_or_else(|| Error::Internal(format!("command {id} vanished")))?;
    Ok(CommandCtx { id, hash })
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

/// The root checkpoint's commit and the import summary of a create or import that succeeded,
/// `None` for a new folder.
fn made(reply: MatterReply) -> Result<(CommitId, Option<ImportSummary>), Error> {
    match reply {
        MatterReply::Created(result) => result.map(|commit| (commit, None)),
        MatterReply::Imported(result) => match result? {
            ImportEnd::Done {
                commit,
                summary,
                warnings,
            } => {
                for warning in &warnings {
                    tracing::info!(?warning, "import warning");
                }
                Ok((commit, Some(summary)))
            }
            ImportEnd::Cancelled => Err(Error::Closed),
        },
        MatterReply::Opened(_)
        | MatterReply::Located(_)
        | MatterReply::BeforeTurn(_)
        | MatterReply::AfterTurn(_)
        | MatterReply::Undone(_)
        | MatterReply::Restored(_)
        | MatterReply::Compared(_)
        | MatterReply::Previewed(_)
        | MatterReply::HeldOpen(_)
        | MatterReply::Papers(_) => Err(Error::Internal(
            "a matter's create got another job's reply".to_owned(),
        )),
    }
}
