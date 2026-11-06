//! Queries: answered from host memory (`ListMatters`, `ListThreads`, `GetSettings`,
//! `ListAgents`, `GetDefaultLocation`, `GetUpdate`), a
//! loaded stream (`PageEntries`), a loaded stream and then the matter's thread
//! (`CompareVersions`, with a version or the folder now, and `PreviewRestore`) or a store
//! read on a task of its own (`ReadBlob`); `ListPapers` runs on the matter's worker
//! (`papers.rs`), and `PreviewUndo` reads the thread's log and then asks the matter's worker
//! (`turn/undo_preview.rs`). `FolderSupport` checks a picked folder off the actor
//! (`query/folder_support.rs`), and `ListSkillPacks` reads the skill packs off it
//! (`query/skill_packs.rs`).

use clauseo_matters::CompareSide;
use clauseo_protocol::{
    AgentId, AgentKind, AgentSummary, CheckpointId, ErrorCode, FolderState, HostQuery, MatterId,
    Principal, QueryResult, RestorePreview, StreamId, UndoPreview, ViewEntry,
};

use std::path::PathBuf;
use std::sync::Arc;

use super::Actor;
use super::folder_state::folder_states;
use super::input::{Input, MatterTag, Reply};
use super::streams::StreamSlot;
use futures::task::LocalSpawnExt;

use crate::blob_read;
use crate::error::Error;
use crate::log_file::Cause;
use crate::matter_worker::{MatterJob, MatterReply};
use crate::principal::{Access, authorize};

mod folder_support;
mod skill_packs;

/// Where new matters go, for display (`GetDefaultLocation`).
/// With none (no Documents folder, issue #365) it answers `command.invalid`, and the form asks
/// for a folder, as it does when the read fails.
fn default_location(actor: &Actor, reply: Reply<QueryResult>) {
    let answer = match actor.effective.default_matters_dir() {
        Some(location) => {
            let display = super::rail::folder_display(location, &actor.home_spellings);
            Ok(QueryResult::DefaultLocation(
                clauseo_protocol::DefaultLocation { display },
            ))
        }
        None => Err(super::matters::no_default_location()),
    };
    let _gone = reply.send(answer);
}

/// A query the host does not answer yet (`Protection`: decision record 0051 answers it once the
/// sandbox seams come, in its next step): `command.invalid`.
fn not_built(reply: Reply<QueryResult>) {
    let refusal = Error::refused(ErrorCode::CommandInvalid, "not built yet");
    let _gone = reply.send(Err(refusal));
}

/// Answers one query.
pub(super) fn query(
    actor: &mut Actor,
    principal: &Principal,
    query: &HostQuery,
    reply: Reply<QueryResult>,
) {
    if let Err(error) = authorize(principal, Access::Query) {
        let _gone = reply.send(Err(error));
        return;
    }
    match *query {
        HostQuery::ListMatters => list_matters(actor, reply),
        HostQuery::ListThreads { matter } => {
            let listed = actor
                .check_matter(matter)
                .map(|_| super::rail::thread_summaries(actor, matter));
            let _gone = reply.send(listed.map(QueryResult::Threads));
        }
        HostQuery::PageEntries {
            stream,
            before_order,
            limit,
        } => {
            if let Err(error) = actor.check_stream(stream) {
                let _gone = reply.send(Err(error));
                return;
            }
            actor.with_loaded(
                stream,
                Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
                    let page = loaded.and_then(|()| match actor.streams.get(&stream) {
                        Some(StreamSlot::Loaded(loaded)) => Ok(QueryResult::Entries(
                            clauseo_reducer::page_entries(&loaded.state, before_order, limit),
                        )),
                        Some(StreamSlot::Loading(_)) | None => {
                            Err(Error::Internal(format!("{stream} is not loaded")))
                        }
                    });
                    let _gone = reply.send(page);
                    actor.release(stream);
                }),
            );
        }
        HostQuery::CompareVersions { matter, from, to } => {
            // `to: None` is the folder now: nothing is stored, and only `from` is checked.
            let (versions, to) = match to {
                Some(to) => (vec![from, to], CompareSide::Checkpoint(to)),
                None => (vec![from], CompareSide::Folder),
            };
            matter_query(
                actor,
                matter,
                versions,
                MatterJob::Compare { from, to },
                reply,
            );
        }
        HostQuery::PreviewRestore {
            matter,
            checkpoint,
            ref scope,
        } => {
            if let Err(invalid) = scope.check("PreviewRestore") {
                let _gone = reply.send(Err(Error::Invalid(invalid)));
                return;
            }
            let job = MatterJob::Preview {
                from: checkpoint,
                scope: scope.clone(),
            };
            matter_query(actor, matter, vec![checkpoint], job, reply);
        }
        HostQuery::PreviewUndo { thread, turn } => {
            super::turn::undo_preview::preview_undo(actor, thread, turn, reply);
        }
        HostQuery::Search(ref request) => actor.search.query(
            crate::search::QUERY_WINDOW,
            request.clone(),
            Box::new(move |answer| {
                let _gone = reply.send(answer.map(QueryResult::Search));
            }),
        ),
        HostQuery::FolderSupport { ref source } => {
            folder_support::folder_support(actor, source, reply);
        }
        HostQuery::GetDefaultLocation => default_location(actor, reply),
        HostQuery::GetSettings => {
            let view = super::settings::view(actor);
            let _gone = reply.send(Ok(QueryResult::Settings(view)));
        }
        HostQuery::GetUpdate => super::update::answer(actor, reply),
        HostQuery::ListSkillPacks => skill_packs::list(actor, reply),
        HostQuery::Protection => not_built(reply),
        HostQuery::ListPapers { matter } => {
            if let Err(error) = actor.check_matter(matter) {
                let _gone = reply.send(Err(error));
                return;
            }
            super::papers::list(actor, matter, reply);
        }
        HostQuery::ListAgents => {
            let agents = list_agents(&actor.effective.settings().agents);
            let _gone = reply.send(Ok(QueryResult::Agents(agents)));
        }
        HostQuery::ReadBlob { hash, offset, len } => {
            let store = actor.store.clone();
            let spawned = actor.spawner.spawn_local(async move {
                let read = store
                    .read_blob(hash, offset.get(), blob_read::request_len(len))
                    .await
                    .map_err(Error::Store)
                    .and_then(|bytes| blob_read::cut(offset, len, &bytes))
                    .map(QueryResult::Blob);
                let _gone = reply.send(read);
            });
            if let Err(error) = spawned {
                tracing::error!(%error, "the host pool refused a blob read");
            }
        }
    }
}

/// One summary per settings agent whose id is a valid `AgentId`, sorted by id (the map's
/// order), with its kind. An id `AgentId` refuses could never name a thread, so it is left out.
fn list_agents<T>(agents: &std::collections::BTreeMap<String, T>) -> Vec<AgentSummary> {
    agents
        .keys()
        .filter_map(|id| AgentId::new(id.as_str()).ok())
        .map(|id| AgentSummary {
            kind: AgentKind::of(&id),
            id,
        })
        .collect()
}

/// A query R8 answers (`CompareVersions`, `PreviewRestore`): checks on the loaded matter
/// stream that each of `versions` is the matter's, then sends `job` to the matter's thread,
/// whose answer goes to `reply` (`matters.rs`).
fn matter_query(
    actor: &mut Actor,
    matter: MatterId,
    versions: Vec<CheckpointId>,
    job: MatterJob,
    reply: Reply<QueryResult>,
) {
    if let Err(error) = actor.check_matter(matter) {
        let _gone = reply.send(Err(error));
        return;
    }
    let stream = StreamId::Matter(matter);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let checked = loaded
                .and_then(|()| own_versions(actor, matter, &versions))
                .and_then(|()| actor.matter_worker(matter));
            match checked {
                // A refused send drops the tag and with it `reply`: the caller sees the host
                // closed.
                Ok(worker) => match worker.send(job, MatterTag::Query(reply)) {
                    Ok(()) => actor.work.matter_jobs += 1,
                    Err(error) => {
                        tracing::warn!(%matter, error = %Cause(&error), "a query was not sent");
                    }
                },
                Err(error) => {
                    let _gone = reply.send(Err(error));
                }
            }
            actor.release(stream);
        }),
    );
}

/// The answer to a query R8 answered (`MatterTag::Query`): `CompareVersions`,
/// `PreviewRestore`, `PreviewUndo`.
pub(super) fn matter_answer(reply: MatterReply) -> Result<QueryResult, Error> {
    match reply {
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
        MatterReply::EditedSince(result) => {
            result.map(|edited_since| QueryResult::UndoPreview(UndoPreview { edited_since }))
        }
        other @ (MatterReply::Created(_)
        | MatterReply::Imported(_)
        | MatterReply::Opened(_)
        | MatterReply::BeforeTurn(_)
        | MatterReply::AfterTurn(_)
        | MatterReply::Undone(_)
        | MatterReply::Restored(_)
        | MatterReply::Located(_)
        | MatterReply::HeldOpen(_)
        | MatterReply::StillGone(_)
        | MatterReply::Papers(_)
        | MatterReply::Discarded(_)) => Err(Error::Internal(format!(
            "a query answered with another job's reply: {other:?}"
        ))),
    }
}

/// `ListMatters`: the summaries from memory (`rail.rs`), each folder's state read off the actor
/// (`folder_state.rs`), answered from a task of its own, which then posts the states back
/// ([`on_folder_states`]).
fn list_matters(actor: &Actor, reply: Reply<QueryResult>) {
    let mut matters = super::rail::summaries(actor);
    let paths: Vec<String> = matters
        .iter()
        .map(|summary| {
            actor
                .matters
                .get(&summary.matter)
                .map(|entry| entry.row.folder_path.clone())
                .unwrap_or_default()
        })
        .collect();
    let folders: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let clock = Arc::clone(&actor.clock);
    let running = Arc::clone(&actor.stats_running);
    actor.spawn_post(async move {
        let states = folder_states(folders, clock, running).await;
        let mut read = Vec::with_capacity(matters.len());
        for ((summary, state), path) in matters.iter_mut().zip(states).zip(paths) {
            summary.folder_state = state;
            read.push((summary.matter, path, state));
        }
        let _gone = reply.send(Ok(QueryResult::Matters(matters)));
        Input::FolderStates(read)
    });
}

/// A `ListMatters` read came back: keep each listed matter's folder state for the app rows,
/// and compare every row at the next flush when one changed. A state read of a folder the
/// matter's row no longer names (a relocate, or a forget and an import, committed while the
/// read was in flight) is dropped: it says nothing of the folder the row names now.
pub(super) fn on_folder_states(actor: &mut Actor, states: Vec<(MatterId, String, FolderState)>) {
    let mut changed = false;
    for (matter, path, state) in states {
        let current = actor
            .matters
            .get(&matter)
            .is_some_and(|entry| entry.row.folder_path == path);
        if !current {
            continue;
        }
        let before = actor.folder_states.insert(matter, state);
        changed |= before.unwrap_or(FolderState::Found) != state;
    }
    if changed {
        actor.lists.touch_all();
    }
}

/// The folder check (`agent/folder_support.rs`) of `matter` answered `support` off the actor,
/// at its create or at an agent launch: keep it for the matter's row (`None`, a fine folder, as
/// no entry), and compare every row at the next flush when it changed.
pub(crate) fn on_folder_support(
    actor: &mut Actor,
    matter: MatterId,
    support: Option<clauseo_protocol::FolderSupportKind>,
) {
    let before = match support {
        Some(kind) => actor.folder_supports.insert(matter, kind),
        None => actor.folder_supports.remove(&matter),
    };
    if before != support {
        actor.lists.touch_all();
    }
}

/// `command.invalid` unless every checkpoint is a version in `matter`'s loaded timeline.
pub(super) fn own_versions(
    actor: &Actor,
    matter: MatterId,
    checkpoints: &[CheckpointId],
) -> Result<(), Error> {
    let state = actor
        .loaded(StreamId::Matter(matter))
        .and_then(|loaded| loaded.state.as_matter())
        .ok_or_else(|| Error::Internal(format!("matter {matter} is not loaded")))?;
    for &checkpoint in checkpoints {
        let known = state.entries().values().any(|entry| {
            matches!(entry, ViewEntry::Version(version) if version.checkpoint == checkpoint)
        });
        if !known {
            return Err(Error::refused(
                ErrorCode::CommandInvalid,
                format!("checkpoint {checkpoint} is not a version of this matter"),
            ));
        }
    }
    Ok(())
}
