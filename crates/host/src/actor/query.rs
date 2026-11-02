//! Queries: answered from host memory (`ListMatters`, `ListThreads`, `GetSettings`,
//! `ListAgents`), a
//! loaded stream (`PageEntries`), a loaded stream and then the matter's thread
//! (`CompareVersions`, with a version or the folder now, and `PreviewRestore`) or a store
//! read on a task of its own (`ReadBlob`); `ListPapers` runs on the matter's worker
//! (`papers.rs`).

use clauseo_matters::CompareSide;
use clauseo_protocol::{
    AgentId, AgentKind, AgentSummary, CheckpointId, CompactString, ErrorCode, FolderState,
    HostQuery, MatterId, MatterSummary, Principal, QueryResult, StreamId, ThreadStatus,
    ThreadSummary, ViewEntry,
};

use std::path::PathBuf;
use std::sync::Arc;

use super::Actor;
use super::folder_state::folder_states;
use super::input::{MatterTag, Reply};
use super::streams::StreamSlot;
use futures::task::LocalSpawnExt;

use crate::blob_read;
use crate::error::Error;
use crate::matter_worker::MatterJob;
use crate::principal::{Access, authorize};

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
                .map(|_| list_threads(actor, matter));
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
        HostQuery::GetSettings => {
            let view = super::settings::view(&actor.effective);
            let _gone = reply.send(Ok(QueryResult::Settings(view)));
        }
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
                    Err(error) => tracing::warn!(%matter, %error, "a query was not sent"),
                },
                Err(error) => {
                    let _gone = reply.send(Err(error));
                }
            }
            actor.release(stream);
        }),
    );
}

/// `ListMatters`: the rows from memory, each folder's state read off the actor
/// (`folder_state.rs`), answered from a task of its own.
fn list_matters(actor: &Actor, reply: Reply<QueryResult>) {
    let (mut matters, folders): (Vec<MatterSummary>, Vec<PathBuf>) = actor
        .matters
        .values()
        .map(|matter| {
            let summary = MatterSummary {
                matter: matter.row.id,
                name: matter.row.name.clone(),
                kind: matter.row.kind,
                folder_state: FolderState::Found,
            };
            (summary, PathBuf::from(&matter.row.folder_path))
        })
        .unzip();
    let clock = Arc::clone(&actor.clock);
    let running = Arc::clone(&actor.stats_running);
    let spawned = actor.spawner.spawn_local(async move {
        let states = folder_states(folders, clock, running).await;
        for (summary, state) in matters.iter_mut().zip(states) {
            summary.folder_state = state;
        }
        let _gone = reply.send(Ok(QueryResult::Matters(matters)));
    });
    if let Err(error) = spawned {
        tracing::error!(%error, "the host pool refused a matter list");
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

/// The threads of `matter`, each with the status of its header: the loaded state's, or
/// the header kept when it was last unloaded, or from its row.
fn list_threads(actor: &Actor, matter: MatterId) -> Vec<ThreadSummary> {
    actor
        .threads
        .values()
        .filter(|thread| thread.row.matter == matter)
        .map(|thread| {
            let loaded = actor
                .loaded(StreamId::Thread(thread.row.id))
                .and_then(|loaded| loaded.state.as_thread())
                .and_then(|state| state.header());
            let header = loaded.or(thread.header.as_ref());
            let row_status = if thread.row.archived {
                ThreadStatus::Archived
            } else {
                ThreadStatus::Idle
            };
            ThreadSummary {
                thread: thread.row.id,
                matter,
                title: header
                    .and_then(|header| header.title.clone())
                    .or_else(|| thread.row.title.as_deref().map(CompactString::from)),
                agent: thread.row.agent.clone(),
                status: header.map_or(row_status, |header| header.status),
                queue_held: false,
            }
        })
        .collect()
}
