//! Queries: answered from host memory (`ListMatters`, `ListThreads`), a loaded stream
//! (`PageEntries`, and `CompareVersions`'s check that both versions are the matter's) or a
//! store read on a task of its own (`ReadBlob`).

use clauseo_protocol::{
    CheckpointId, CompactString, ErrorCode, HostQuery, MatterId, MatterSummary, Principal,
    QueryResult, StreamId, ThreadStatus, ThreadSummary, ViewEntry,
};

use super::Actor;
use super::input::Reply;
use super::streams::StreamSlot;
use futures::task::LocalSpawnExt;

use crate::blob_read;
use crate::error::Error;
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
        HostQuery::ListMatters => {
            let matters = actor
                .matters
                .values()
                .map(|matter| MatterSummary {
                    matter: matter.row.id,
                    name: matter.row.name.clone(),
                    kind: matter.row.kind,
                })
                .collect();
            let _gone = reply.send(Ok(QueryResult::Matters(matters)));
        }
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
            if let Err(error) = actor.check_matter(matter) {
                let _gone = reply.send(Err(error));
                return;
            }
            let stream = StreamId::Matter(matter);
            actor.with_loaded(
                stream,
                Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
                    let answer = loaded
                        .and_then(|()| own_versions(actor, matter, [from, to]))
                        .and_then(|()| {
                            Err(Error::refused(
                                ErrorCode::CommandInvalid,
                                "comparing versions is not built yet",
                            ))
                        });
                    let _gone = reply.send(answer);
                    actor.release(stream);
                }),
            );
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

/// `command.invalid` unless every checkpoint is a version in `matter`'s loaded timeline.
fn own_versions(
    actor: &Actor,
    matter: MatterId,
    checkpoints: [CheckpointId; 2],
) -> Result<(), Error> {
    let state = actor
        .loaded(StreamId::Matter(matter))
        .and_then(|loaded| loaded.state.as_matter())
        .ok_or_else(|| Error::Internal(format!("matter {matter} is not loaded")))?;
    for checkpoint in checkpoints {
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
