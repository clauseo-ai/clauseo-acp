//! A matter made on disk whose record failed. A create or import writes the folder's
//! `.clauseo/matter.json` and the matter's history on its worker, then the actor commits the
//! matter's events and row; when building or storing that commit fails, the folder would keep a
//! marker naming a matter Clauseo has no row for, and every later "New matter" there would be
//! refused as already a matter. So the worker discards what the create or import made
//! (`Matter::discard`: the history, `matter.json`, and `.clauseo` or the folder only when it
//! made them) and only then is the command answered, with the commit's own error, so a retry
//! finds the folder as it was.
//!
//! Only a matter whose row is surely not stored is discarded: one whose commit could not be built
//! or sent (the store never had it). A commit the store answered with an error may still have
//! been stored (`COMMIT` failing after its frames were written, a reply lost on the way), so
//! the store is asked first, up to [`LOOKUPS`] times: with no row for the matter, it is
//! discarded; with one, or when every lookup fails, everything is kept and the command gets the
//! commit's error. Losing nothing comes before a clean retry. What is left then: a stored row
//! is listed at the next start, but when the store could not say and the row was in fact not
//! stored, the folder keeps a marker and the data folder a history with no row, as before this
//! rollback, and a new import of the folder is refused as already a matter.
//!
//! The logs here name the matter, the error's code and the stage, never a folder: a path names a
//! client.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clauseo_matters::CommitId;
use clauseo_protocol::{
    CheckpointKind, CheckpointRecorded, CommandId, CommandOutcome, DomainEvent, ImportSummary,
    MatterCreated, MatterId, StreamId,
};
use clauseo_store::MatterRow;
use futures::task::LocalSpawnExt;

use super::super::input::{Input, MatterTag};
use super::super::{Actor, MatterEntry, turn};
use super::{PendingMatter, command_ctx};
use crate::error::Error;
use crate::matter_worker::{MatterJob, MatterReply, MatterWorker, matter_settings};
use crate::pipeline::{Draft, MatterEvent};

/// A made matter's worker, until its commit returns; with the failure to answer, once a discard
/// was sent.
pub(crate) struct Made {
    worker: MatterWorker<MatterTag>,
    /// The matter's folder, for the marker's re-check after a discard ([`recheck_marker`]).
    folder: PathBuf,
    /// The command to answer (none for the Scratch matter) and its error.
    failed: Option<(Option<CommandId>, Error)>,
}

impl Made {
    pub(crate) const fn new(worker: MatterWorker<MatterTag>, folder: PathBuf) -> Self {
        Self {
            worker,
            folder,
            failed: None,
        }
    }
}

/// The commit of a made matter returned: it joins the list with its worker.
pub(crate) fn recorded(actor: &mut Actor, mut entry: MatterEntry) {
    entry.worker = actor.made.remove(&entry.row.id).map(|made| made.worker);
    actor.matters.insert(entry.row.id, entry);
}

/// Where a made matter's record failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// Its commit could not be built or sent: the store never had it.
    Commit,
    /// The store answered its commit with an error: the row may be stored all the same.
    Store,
}

/// The made matter `matter` was not recorded at `stage`. For [`Stage::Commit`] its worker
/// discards it; for [`Stage::Store`] the store is asked for its row first ([`row_checked`]).
/// Command `command` is answered with `error` once that is done.
pub(crate) fn discard(
    actor: &mut Actor,
    command: Option<CommandId>,
    matter: MatterId,
    error: Error,
    stage: Stage,
) {
    tracing::warn!(%matter, code = %error.code(), ?stage, "a matter made on disk was not recorded");
    let Some(made) = actor.made.get_mut(&matter) else {
        answer(actor, command, error);
        return;
    };
    made.failed = Some((command, error));
    match stage {
        Stage::Commit => send_discard(actor, matter),
        Stage::Store => check_row(actor, matter),
    }
}

/// How many times the store is asked for a made matter's row before it is kept.
const LOOKUPS: u32 = 3;

/// Asks the store whether it holds `matter`'s row, up to [`LOOKUPS`] times with a short wait
/// (50 ms, then 200 ms) after each failure; the last answer comes as [`Input::MadeRow`].
fn check_row(actor: &mut Actor, matter: MatterId) {
    let (store, post, clock) = (
        actor.store.clone(),
        actor.post.clone(),
        std::sync::Arc::clone(&actor.clock),
    );
    let asked = actor.spawner.spawn_local(async move {
        let mut tries = 0;
        let result = loop {
            tries += 1;
            let found = store.matters().await;
            match found {
                Err(error) if tries < LOOKUPS => {
                    tracing::debug!(%matter, code = %error.code(), tries, "row lookup failed");
                    clock
                        .after(Duration::from_millis(50 * 4_u64.pow(tries - 1)))
                        .await;
                }
                found => break found.map(|rows| rows.iter().any(|row| row.id == matter)),
            }
        };
        let _gone = post.try_send(Input::MadeRow { matter, result });
    });
    match asked {
        // Counted as a matter job, so shutdown waits for it.
        Ok(()) => actor.work.matter_jobs += 1,
        Err(_shutdown) => keep(actor, matter, "the store could not be asked"),
    }
}

/// The store answered whether it holds `matter`'s row: discarded only when it surely does not.
pub(crate) fn row_checked(
    actor: &mut Actor,
    matter: MatterId,
    result: Result<bool, clauseo_store::Error>,
) {
    actor.work.matter_jobs = actor.work.matter_jobs.saturating_sub(1);
    match result {
        Ok(false) => send_discard(actor, matter),
        Ok(true) => keep(actor, matter, "its row was stored"),
        Err(error) => {
            tracing::warn!(%matter, code = %error.code(), stage = "lookup", "lookup failed");
            keep(actor, matter, "the store could not say");
        }
    }
}

/// Keeps what the create or import made and answers its command with the commit's error.
fn keep(actor: &mut Actor, matter: MatterId, why: &'static str) {
    tracing::warn!(%matter, why, "the matter made on disk is kept");
    if let Some(made) = actor.made.remove(&matter)
        && let Some((command, error)) = made.failed
    {
        answer(actor, command, error);
    }
}

/// Sends `matter`'s worker its discard; the answer comes in [`discarded`]. The folder check's
/// answer for it is dropped here (a matter kept keeps it): no row will show it.
fn send_discard(actor: &mut Actor, matter: MatterId) {
    actor.folder_supports.remove(&matter);
    let Some(made) = actor.made.get(&matter) else {
        return;
    };
    match made.worker.send(MatterJob::Discard, MatterTag::Discard) {
        Ok(()) => actor.work.matter_jobs += 1,
        Err(closed) => {
            tracing::warn!(%matter, code = %closed.code(), stage = "discard", "not discarded");
            if let Some(Made {
                failed: Some((command, error)),
                ..
            }) = actor.made.remove(&matter)
            {
                answer(actor, command, error);
            }
        }
    }
}

/// The discard of `matter` returned: logged when it failed, then the command is answered with
/// the commit's error.
pub(crate) fn discarded(actor: &mut Actor, matter: MatterId, reply: &MatterReply) {
    if let MatterReply::Discarded(Err(error)) = reply {
        tracing::warn!(%matter, code = %error.code(), stage = "discard", "not discarded");
    } else if !matches!(reply, MatterReply::Discarded(Ok(()))) {
        tracing::error!(%matter, "a discard answered with another job's reply");
    }
    let Some(made) = actor.made.remove(&matter) else {
        return;
    };
    // Joins the worker, idle now that its last job has answered.
    drop(made.worker);
    if matches!(reply, MatterReply::Discarded(Ok(()))) {
        recheck_marker(actor, matter, made.folder);
    }
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
    actor
        .made
        .insert(new.id, Made::new(worker, new.folder.clone()));
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
        skill_packs: None,
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

/// How long after a cancelled, failed or discarded create or import the folder's marker is
/// looked at again.
pub(crate) const MARKER_RECHECK: Duration = Duration::from_secs(10);

/// Once, [`MARKER_RECHECK`] from now: a sync client can put back the `.clauseo/matter.json` a
/// cancelled, failed or discarded create or import of `matter` removed, and every later "New
/// matter" there would then be refused as already a matter. Removed again (with `.clauseo`
/// when that leaves it empty) only if it names `matter`, the id minted for that create or
/// import, which no row holds; a marker naming any other matter, one this host knows or one
/// from another computer, is left alone. Off the actor; never repeated. Shutdown does not wait
/// for it.
pub(super) fn recheck_marker(actor: &Actor, matter: MatterId, folder: PathBuf) {
    let clock = Arc::clone(&actor.clock);
    let task = async move {
        clock.after(MARKER_RECHECK).await;
        let job = crate::agent::codex::prepare::Job::short("marker re-check", Some(matter));
        let removed = crate::agent::codex::prepare::off_actor(clock, job, move || {
            clauseo_matters::remove_marker_of(&folder, matter).map_err(Error::from)
        })
        .await;
        match removed {
            Ok(true) => tracing::warn!(
                %matter,
                files = 1,
                "a matter.json put back after its create or import was undone was removed"
            ),
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%matter, code = %error.code(), stage = "recheck", "not checked");
            }
        }
    };
    if let Err(error) = actor.spawner.spawn_local(task) {
        tracing::error!(%error, "the host pool refused a marker re-check");
    }
}
