//! A `CreateMatter`'s create or import answered on its worker: the matter made is committed, a
//! forgotten matter's folder brings it back, and a refusal is worded for the form. An import
//! whose folder something else answers for makes nothing of its own ([`settle`]):
//!
//! - a listed matter's own folder, by any spelling: answered with that matter as a forgotten
//!   matter's folder is (`MatterCreated` naming it), with a receipt and nothing else: no new
//!   matter, no count, no history touched;
//! - a folder another create or import in flight is making: the command joins that one and gets
//!   its answer, whatever it is ([`join`]);
//! - a folder whose marker names a matter this install has no record of (its data was wiped, or
//!   the folder is a colleague's): imported again under that id, on a new worker of that id,
//!   with the same claim, cancel flag and details. The marker is kept as it is, and a failed or
//!   cancelled adopting import leaves it byte for byte: [`made::recheck_marker`] is skipped for
//!   it, since the id it would remove is the one the lawyer's marker names.
//!
//! The actor issues the import again rather than letting the worker switch ids: a worker's
//! replies carry the id it was started with (`start_worker`), which its matter keeps for life.
//! When the state moved on between the registry's snapshot and the answer (the listed matter
//! was forgotten, the command to join has ended, the marker's matter became known), the import
//! simply runs again on what is there now ([`again`]).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use clauseo_matters::Refusal;
use clauseo_protocol::{CommandId, CommandOutcome, MatterId, StreamId};

use super::super::commands::AfterCommand;
use super::super::input::MatterTag;
use super::super::{Actor, Tag};
use super::{
    PendingMatter, begin_create, claimed_folders, command_ctx, made, matter_commit, refusal,
    refused_folders, start_worker,
};
use crate::error::Error;
use crate::matter_worker::{ImportEnd, MatterJob, MatterReply};
use crate::pipeline::Draft;

/// Command `id`'s create or import, made as `matter` on its worker, answered with `reply`.
pub(super) fn create_replied(
    actor: &mut Actor,
    matter: MatterId,
    id: CommandId,
    reply: MatterReply,
) {
    let pending = actor
        .commands
        .get_mut(&id)
        .and_then(|command| command.matter.take());
    let Some(pending) = pending else {
        return;
    };
    let Some((pending, reply)) = settle(actor, id, pending, reply) else {
        return;
    };
    let outcome = CommandOutcome::MatterCreated { matter };
    let minted = pending.new.id;
    let folder = pending.new.folder.clone();
    let adopted = pending.adopted;
    let committed = match crate::matter_worker::made(reply) {
        Err(Error::Matters(clauseo_matters::Error::FolderRefused {
            reason: Refusal::AlreadyMatter(known),
            ..
        })) if super::super::matter_details::is_forgotten(actor, known) => {
            let details = pending.details.clone();
            command_ctx(actor, id).and_then(|ctx| {
                super::super::matter_details::unforget(actor, &ctx, known, &folder, details)
            })
        }
        Err(error) => Err(error),
        Ok((import, support)) => {
            // Made on disk from here: a failure discards it before the answer.
            let opened = super::super::counting::matter_created(import.1.as_ref(), folder);
            let sent = matter_commit(actor, *pending, import, Some((id, outcome))).and_then(
                |(draft, entry)| {
                    let after = AfterCommand::Matter(Box::new(entry));
                    actor.send_commit(draft, Tag::Command { id, outcome, after })
                },
            );
            match sent {
                Ok(_) => {
                    // Kept once the commit is sent, so a matter discarded before it has no
                    // entry; a store failure after it drops it in the discard.
                    super::super::query::on_folder_support(actor, minted, support);
                    super::super::counting::on_commit(actor, id, opened);
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
        if !adopted {
            made::recheck_marker(actor, minted, folder);
        }
        actor.finish_command(id, Err(error));
    }
}

/// Answers command `id` when its import's folder is held ([`ImportEnd::Listed`],
/// [`ImportEnd::Making`], [`ImportEnd::Adopt`]); otherwise gives `pending` and `reply` back.
fn settle(
    actor: &mut Actor,
    id: CommandId,
    pending: Box<PendingMatter>,
    reply: MatterReply,
) -> Option<(Box<PendingMatter>, MatterReply)> {
    let MatterReply::Imported(Ok(end)) = &reply else {
        return Some((pending, reply));
    };
    match *end {
        ImportEnd::Listed(listed) => listed_folder(actor, id, pending, listed),
        ImportEnd::Making(other) => {
            if join(actor, id, other) {
                tracing::info!(command = %id, joined = %other, "a create joined another");
            } else {
                again(actor, id, *pending);
            }
        }
        ImportEnd::Adopt(old) => adopt(actor, id, pending, old),
        ImportEnd::Done { .. } | ImportEnd::Cancelled => return Some((pending, reply)),
    }
    None
}

/// The import's folder is the listed matter `listed`'s: command `id` is answered with it by a
/// receipt alone, as an import of a forgotten matter's folder is answered with that matter.
fn listed_folder(actor: &mut Actor, id: CommandId, pending: Box<PendingMatter>, listed: MatterId) {
    if !actor.matters.contains_key(&listed) {
        again(actor, id, *pending);
        return;
    }
    // Joins the minted id's worker, idle: it made nothing.
    drop(pending);
    tracing::info!(command = %id, matter = %listed, "an import of a listed matter's folder");
    let sent = command_ctx(actor, id).and_then(|ctx| {
        let outcome = CommandOutcome::MatterCreated { matter: listed };
        let now = actor.now().wall;
        let mut draft = Draft::new();
        draft.receipt(ctx.receipt(Some(StreamId::Matter(listed)), outcome, now));
        let after = AfterCommand::Nothing;
        actor.send_commit(draft, Tag::Command { id, outcome, after })
    });
    if let Err(error) = sent {
        actor.finish_command(id, Err(error));
    }
}

/// The import's folder has a marker naming `old`, a matter the registry did not hold and with no
/// history here: imported again under `old`. When `old` is listed or forgotten by now, the
/// import runs again and finds it; when the host is stopping it ends closed. It is refused as
/// already a matter when this import was already issued again under an adopted id (the marker
/// changed between the two), or when `old` is being made, discarded or moved by another command
/// (running again would loop until that command ends).
fn adopt(actor: &mut Actor, id: CommandId, pending: Box<PendingMatter>, old: MatterId) {
    if actor.matters.contains_key(&old) || actor.forgotten.contains_key(&old) {
        again(actor, id, *pending);
        return;
    }
    let busy = pending.adopted
        || actor.made.contains_key(&old)
        || actor.claims.values().any(|(matter, _)| *matter == old)
        || actor
            .scratch
            .as_ref()
            .is_some_and(|scratch| scratch.new.id == old);
    let result = if pending.cancel.load(Ordering::Relaxed) {
        Err(Error::Closed)
    } else if busy {
        let reason = Refusal::AlreadyMatter(old);
        let folder = pending.new.folder.clone();
        let refused = Error::Matters(clauseo_matters::Error::FolderRefused { folder, reason });
        Err(refusal(actor, refused, &pending.new.folder))
    } else {
        Ok(())
    };
    let minted = pending.new.id;
    if let Err(error) = result.and_then(|()| reissue(actor, id, pending, old)) {
        made::not_created(Some(minted), &error, "adopt");
        actor.finish_command(id, Err(error));
    }
}

/// Issues command `id`'s import again under `old`, on a new worker of that id, with the folder
/// claimed under `old` until the command ends.
fn reissue(
    actor: &mut Actor,
    id: CommandId,
    mut pending: Box<PendingMatter>,
    old: MatterId,
) -> Result<(), Error> {
    let worker = start_worker(actor, old)?;
    // The registry without this command's own claim, which it would otherwise join.
    let registry = claimed_folders(actor)
        .into_iter()
        .filter(|claimed| claimed.command != Some(id))
        .collect();
    pending.new.id = old;
    let job = MatterJob::Import {
        new: pending.new.clone(),
        refused: refused_folders(actor),
        registry,
        cancel: Arc::clone(&pending.cancel),
        check: actor.folder_check,
    };
    worker.send(job, MatterTag::Create(id))?;
    actor.work.matter_jobs += 1;
    // The minted id's worker, idle, is joined here.
    pending.worker = worker;
    pending.adopted = true;
    actor.claims.insert(id, (old, pending.new.folder.clone()));
    if let Some(command) = actor.commands.get_mut(&id) {
        command.matter = Some(pending);
    }
    tracing::info!(command = %id, matter = %old, "an import adopts the matter its marker names");
    Ok(())
}

/// Runs command `id`'s import of `pending`'s folder again from the start, on the state now.
fn again(actor: &mut Actor, id: CommandId, pending: PendingMatter) {
    let PendingMatter {
        new, name, details, ..
    } = pending;
    actor.claims.remove(&id);
    begin_create(actor, id, (name, details), new.folder, true);
}

/// Joins command `id` to command `to`, which makes the same matter folder: `id`'s callers wait
/// for `to`'s answer and get it as `to`'s duplicates do (`commands::finish_command`), and `id`
/// is dropped with its claim, unanswered and with no receipt (a retry runs again and finds the
/// matter listed). False, changing nothing, when `to` is `id` or either is not pending.
pub(super) fn join(actor: &mut Actor, id: CommandId, to: CommandId) -> bool {
    if id == to || !actor.commands.contains_key(&to) {
        return false;
    }
    let Some(joining) = actor.commands.remove(&id) else {
        return false;
    };
    actor.claims.remove(&id);
    if let Some(target) = actor.commands.get_mut(&to) {
        target.waiters.extend(joining.waiters);
    }
    true
}
