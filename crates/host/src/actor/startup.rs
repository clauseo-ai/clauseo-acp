//! Start-up: open the store, load the registry, start the feeder and the snapshot thread, then
//! (in the loop) create the Scratch matter on the first start and reconcile every thread.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::mpsc;

use clauseo_protocol::{MatterKind, StreamId};
use clauseo_store::{Store, StoreOptions};
use futures::executor::LocalSpawner;

use super::input::Input;
use super::{Actor, MatterEntry, Phase, ThreadEntry, Work, turn};
use crate::config::HostConfig;
use crate::encode::Encoder;
use crate::error::Error;
use crate::ids::Ids;
use crate::matter_worker::matter_settings;
use crate::pipeline::{Pipeline, Post, spawn_feeder};
use crate::snapshot::SnapshotWorker;
use crate::write;

/// Opens what the actor owns. Every read here awaits directly: the loop has not started.
pub(super) async fn open(
    config: HostConfig,
    spawner: LocalSpawner,
    post: async_channel::Sender<Input>,
    ready: mpsc::Sender<Result<(), Error>>,
) -> Result<Actor, Error> {
    let parts = config.into_parts();
    let data_dir = parts.effective.data_dir().to_path_buf();
    write::create_dir_all(&data_dir).map_err(|error| Error::Store(error.into()))?;
    write::create_dir_all(&matter_settings(&data_dir).matters_root)
        .map_err(|error| Error::Store(error.into()))?;
    let store = Store::open(StoreOptions::new(data_dir)).await?;
    let heads = store.heads().await?;
    let mut matters = BTreeMap::new();
    let mut threads = BTreeMap::new();
    for row in store.matters().await? {
        for thread in store.threads(row.id).await? {
            threads.insert(
                thread.id,
                ThreadEntry {
                    row: thread,
                    header: None,
                    turn: turn::TurnState::default(),
                },
            );
        }
        matters.insert(
            row.id,
            MatterEntry {
                row,
                worker: None,
                turns: turn::MatterTurns::default(),
            },
        );
    }
    let commits = post.clone();
    let feeder_post: Post = Rc::new(move |ticket, result| {
        let _gone = commits.try_send(Input::Committed { ticket, result });
    });
    let feeder = spawn_feeder(&spawner, store.clone(), feeder_post)?;
    let snapshot_post = post.clone();
    let snapshots = SnapshotWorker::start(Box::new(move |snapshot| {
        let _gone = snapshot_post.try_send(Input::Snapshot(snapshot));
    }))?;
    Ok(Actor {
        spawner,
        post,
        store,
        pipeline: Pipeline::new(heads),
        feeder,
        snapshots,
        effective: parts.effective,
        launcher: parts.launcher,
        profile: parts.profile,
        refused_folders: parts.refused_folders,
        clock: parts.clock,
        ids: Ids::new(parts.ids),
        window_cap: parts.patch_window_bytes,
        encoder: Encoder::new(),
        matters,
        threads,
        streams: BTreeMap::new(),
        event_subs: BTreeMap::new(),
        commands: BTreeMap::new(),
        folders: BTreeMap::new(),
        files: BTreeMap::new(),
        next_sub: 0,
        work: Work::default(),
        phase: Phase::Starting(ready),
        scratch: None,
    })
}

/// Queues the start-up work: the Scratch matter when there is none, and the load and
/// reconciliation of every thread that is not archived. The loop answers `start` once
/// all of it is done.
pub(super) fn begin_startup(actor: &mut Actor) {
    let has_scratch = actor
        .matters
        .values()
        .any(|matter| matter.row.kind == MatterKind::Scratch);
    if !has_scratch {
        super::matters::create_scratch(actor);
    }
    let open: Vec<_> = actor
        .threads
        .values()
        .filter(|thread| !thread.row.archived)
        .map(|thread| thread.row.id)
        .collect();
    for thread in open {
        let stream = StreamId::Thread(thread);
        actor.with_loaded(
            stream,
            Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
                match loaded {
                    Ok(()) => turn::reconcile(actor, thread),
                    Err(error) => {
                        tracing::warn!(%error, %thread, "could not load a thread at start-up");
                    }
                }
                actor.release(stream);
            }),
        );
    }
}
