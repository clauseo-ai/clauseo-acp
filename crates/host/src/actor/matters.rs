//! Matters: `CreateMatter` (a new folder, picked or in the default location, or an import), the
//! Scratch matter's create and commit (its folder scan is in `scratch.rs`), and each matter's
//! worker thread, started and opened on
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

use clauseo_matters::{DEFAULT_MARKER, NewMatter, Refusal};
use clauseo_protocol::{
    ClientName, CommandId, CommandOutcome, Date, ErrorCode, HostError, MatterId, MatterKind,
    MatterSource, Title,
};

use super::commands::{AfterCommand, CommandCtx};
use super::input::{Input, MatterTag};
use super::{Actor, MatterEntry, Tag, turn};
use crate::error::Error;
use crate::log_file::Cause;
use crate::matter_worker::{
    Claim, Claimed, MatterJob, MatterReply, MatterWorker, Registry, Relink, check_registry, made,
    matter_settings,
};
use crate::pipeline::Failure;

mod made;
pub(super) mod picked;
pub(super) mod probe;

pub(crate) use made::Made;
use made::matter_commit;
pub(super) use made::{Stage, discard, recorded, row_checked};

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

/// Starts a new matter's worker with its create or import job. The Scratch matter's job is not
/// counted in `work.matter_jobs`: `scratch::holds` holds start-up and shutdown for it instead,
/// so start-up's ready waits for it only so long.
pub(super) fn begin_matter(
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
    let check = actor.folder_check;
    let job = if let Some(registry) = import {
        MatterJob::Import {
            new: new.clone(),
            refused: refused_folders(actor),
            registry,
            cancel: Arc::clone(&cancel),
            check,
        }
    } else {
        MatterJob::Create {
            new: new.clone(),
            check,
        }
    };
    let counted = !matches!(tag, MatterTag::Scratch);
    worker.send(job, tag)?;
    if counted {
        actor.work.matter_jobs += 1;
    }
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
/// commit. A new folder's parent is canonicalised and the folder looked for off the actor
/// (`probe.rs`), as either may hang on a network share; the create goes on when that answers
/// (`begin_create`). A folder that is, holds or sits inside another matter's folder (listed,
/// forgotten or being made; `registry`) is refused with `matter.folder_is_matter`, naming the
/// matter when it is listed: for an import on its worker, after the place check, so a drive root
/// or the home folder is named for what it is even when a matter sits below it; for a new folder
/// on the actor once its probe answers, since only its parent exists.
pub(super) fn create_matter(
    actor: &mut Actor,
    ctx: &CommandCtx,
    name: &Title,
    source: MatterSource,
    details: Details,
) {
    let waits = matches!(source, MatterSource::DefaultLocation);
    if waits && super::documents::hold_create(actor, ctx, name, &details) {
        return;
    }
    let name = name.as_str().to_owned();
    match matter_folder(actor, &source) {
        Ok((folder, true)) => begin_create(actor, ctx.id, (name, details), folder, true),
        Ok((parent, false)) => probe::start(actor, ctx.id, (name, details), parent),
        Err(error) => not_begun(actor, ctx.id, error),
    }
}

/// Starts a `CreateMatter`'s create or import of `folder` on its new worker, checked first
/// against the folders claimed now (for a new folder; an import's worker checks them), and
/// reserves the folder in `actor.claims` and the command's `matter` until the command ends.
/// Both are set here on the actor, so of two creates of one folder the second always finds the
/// first's claim.
pub(super) fn begin_create(
    actor: &mut Actor,
    id: CommandId,
    (name, details): (String, Details),
    folder: PathBuf,
    import: bool,
) {
    let registry = claimed_folders(actor);
    let registry = if import {
        Ok(Some(registry))
    } else {
        check_registry(&folder, &registry, false).map(|()| None)
    };
    let begun = registry.and_then(|import| {
        begin_matter(
            actor,
            (name, MatterKind::Folder, details),
            folder,
            import,
            MatterTag::Create(id),
        )
    });
    match begun {
        Ok(pending) => {
            let claim = (pending.new.id, pending.new.folder.clone());
            actor.claims.insert(id, claim);
            if let Some(command) = actor.commands.get_mut(&id) {
                command.matter = Some(pending);
            }
        }
        Err(error) => not_begun(actor, id, error),
    }
}

/// A `CreateMatter` refused before its matter's worker started.
pub(super) fn not_begun(actor: &mut Actor, id: CommandId, error: Error) {
    made::not_created(None, &error, "check");
    actor.finish_command(id, Err(error));
}

/// The folder a `CreateMatter` names and whether it is an import: an import's folder, or a new
/// folder's parent as registered or configured. Nothing here touches the disk.
fn matter_folder(actor: &Actor, source: &MatterSource) -> Result<(PathBuf, bool), Error> {
    match source {
        MatterSource::NewFolder { folder } => Ok((registered(actor, *folder)?, false)),
        MatterSource::ExistingFolder { folder } => Ok((registered(actor, *folder)?, true)),
        MatterSource::DefaultLocation => Ok((default_location(actor)?, false)),
    }
}

/// The default matters folder, or `command.invalid` when there is none (no Documents folder,
/// issue #365), or the retryable `matter.folder_missing` while Documents is still being looked
/// up past the hold's bound (`documents.rs`: before it, a create waits for the answer).
pub(super) fn default_location(actor: &Actor) -> Result<PathBuf, Error> {
    match super::documents::default_matters_dir(actor) {
        Some(location) => Ok(location.to_path_buf()),
        None if super::documents::pending(actor) => Err(super::documents::not_yet()),
        None => Err(no_default_location()),
    }
}

/// The refusal for a default location the host does not have.
pub(super) fn no_default_location() -> Error {
    Error::refused(
        ErrorCode::CommandInvalid,
        "this host has no default location",
    )
}

/// The folder `folder` names, or `command.invalid` when it names none.
pub(super) fn registered(
    actor: &Actor,
    folder: clauseo_protocol::FolderRefId,
) -> Result<PathBuf, Error> {
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
    RefusalNames::of(actor).refusal(error, folder, |matter| actor.matters.contains_key(matter))
}

/// What [`refusal`] tells the refused folders apart by, taken from the actor, so the
/// `FolderSupport` query names a refusal off the actor as create does (`picked.rs`).
struct RefusalNames {
    home: Option<PathBuf>,
    data_dir: PathBuf,
}

impl RefusalNames {
    fn of(actor: &Actor) -> Self {
        Self {
            home: actor.home.clone(),
            data_dir: actor.effective.data_dir().to_path_buf(),
        }
    }

    /// [`refusal`]'s code for `error`; `listed` says whether a matter is on the list.
    fn refusal(&self, error: Error, folder: &Path, listed: impl Fn(&MatterId) -> bool) -> Error {
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
            Refusal::Listed(entry) if self.home.as_deref() == Some(entry.as_path()) => {
                ErrorCode::MatterFolderIsHome
            }
            Refusal::Listed(entry) if entry.as_path() == self.data_dir => {
                ErrorCode::MatterFolderInMattersRoot
            }
            Refusal::Listed(_) => ErrorCode::MatterFolderIsSystem,
            // Named only when on the list: a marker from another data folder or computer names a
            // matter this install has never had, with nothing to open. (A forgotten matter's marker
            // never gets here: the import brings that matter back.)
            Refusal::AlreadyMatter(matter) => {
                let refusal = HostError::new(ErrorCode::MatterFolderIsMatter, error.to_string());
                return Error::Refused(if listed(matter) {
                    refusal.with_matter(*matter)
                } else {
                    refusal
                });
            }
            Refusal::UnreadableMatterJson => ErrorCode::MatterFolderUnreadable,
            Refusal::MattersRoot => ErrorCode::MatterFolderInMattersRoot,
            Refusal::TooDeep => ErrorCode::MatterFolderTooDeep,
        };
        Error::refused(code, error.to_string())
    }
}

/// A matter worker answered.
pub(super) fn on_matter_reply(
    actor: &mut Actor,
    matter: MatterId,
    tag: MatterTag,
    reply: MatterReply,
) {
    // The Scratch matter's create is not counted (`begin_matter`).
    if !matches!(tag, MatterTag::Scratch) {
        actor.work.matter_jobs = actor.work.matter_jobs.saturating_sub(1);
    }
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
            let minted = pending.new.id;
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
                Err(error) => Err(error),
                Ok((import, support)) => {
                    // Made on disk from here: a failure discards it before the answer.
                    let opened = super::counting::matter_created(import.1.as_ref(), folder);
                    let sent = matter_commit(actor, *pending, import, Some((id, outcome)))
                        .and_then(|(draft, entry)| {
                            let after = AfterCommand::Matter(Box::new(entry));
                            actor.send_commit(draft, Tag::Command { id, outcome, after })
                        });
                    match sent {
                        Ok(_) => {
                            // Kept once the commit is sent, so a matter discarded before it has
                            // no entry; a store failure after it drops it in the discard.
                            super::query::on_folder_support(actor, minted, support);
                            super::counting::on_commit(actor, id, opened);
                        }
                        Err(error) => {
                            made::discard(actor, Some(id), minted, error, made::Stage::Commit);
                        }
                    }
                    return;
                }
            };
            if let Err(error) = committed {
                let error = refusal(actor, error, &folder);
                made::not_created(Some(minted), &error, "worker");
                made::recheck_marker(actor, minted, folder);
                actor.finish_command(id, Err(error));
            }
        }
        MatterTag::Scratch => {
            let Some(pending) = actor.scratch.take() else {
                return;
            };
            match made(reply) {
                Ok((import, _)) => {
                    let sent =
                        matter_commit(actor, *pending, import, None).and_then(|(draft, entry)| {
                            actor.send_commit(draft, Tag::Scratch(Box::new(entry)))
                        });
                    if let Err(error) = sent {
                        made::discard(actor, None, matter, error, made::Stage::Commit);
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %Cause(&error), "could not create the Scratch matter");
                }
            }
        }
        MatterTag::Open => super::matter_details::opened(matter, reply),
        MatterTag::Relocate { command, folder } => {
            super::relocate::located(actor, matter, command, &folder, reply);
        }
        MatterTag::Turn(job) => turn::matter_reply(actor, matter, job, reply),
        MatterTag::Restore(ask) => {
            if let MatterReply::Restored(result) = reply {
                turn::on_restored(actor, &ask, result.map(|done| *done));
            } else {
                tracing::error!(
                    reply = reply.name(),
                    "a restore answered with another job's reply"
                );
            }
        }
        MatterTag::Query(answer) => {
            let _gone = answer.send(super::query::matter_answer(reply));
        }
        MatterTag::Papers(answer) => super::papers::answer(actor, matter, reply, answer),
        MatterTag::Discard => made::discarded(actor, matter, &reply),
        MatterTag::CameBack(check) => turn::came_back_reply(actor, check, reply),
    }
}

/// The receipt context of the pending command `id`.
pub(super) fn command_ctx(
    actor: &Actor,
    id: clauseo_protocol::CommandId,
) -> Result<CommandCtx, Error> {
    let hash = actor
        .commands
        .get(&id)
        .map(|command| command.hash)
        .ok_or_else(|| Error::Internal(format!("command {id} vanished")))?;
    Ok(CommandCtx { id, hash })
}

/// The Scratch matter's commit returned. Once durable it joins the list, and app subscribers
/// hear of it at the next flush, as of a matter a command created: Scratch can come after
/// ready (`scratch.rs`).
pub(super) fn scratch_committed(
    actor: &mut Actor,
    entry: MatterEntry,
    outcome: Result<(), &Failure>,
) {
    match outcome {
        Ok(()) => {
            made::recorded(actor, entry);
            actor.lists.touch_all();
        }
        Err(failure) => {
            let error = Error::Store(failure.error.clone());
            made::discard(actor, None, entry.row.id, error, made::Stage::Store);
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
