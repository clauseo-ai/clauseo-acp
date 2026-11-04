//! Start-up: open the store, load the registry, start the feeder and the snapshot thread and the
//! limits model (its first reads run on their own), then
//! (in the loop) create the Scratch matter on the first start and reconcile every thread.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::mpsc;

use clauseo_protocol::{MatterKind, StreamId};
use clauseo_store::{Store, StoreOptions};
use futures::executor::LocalSpawner;

use super::input::Input;
use super::{Actor, HostPhase, MatterEntry, ThreadEntry, Work, turn};
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
    let tails = store.tail_times().await?.into_iter().collect();
    let mut matters = BTreeMap::new();
    let mut forgotten = BTreeMap::new();
    let mut threads = BTreeMap::new();
    for row in store.matters().await? {
        for thread in store.threads(row.id).await? {
            threads.insert(
                thread.id,
                ThreadEntry {
                    row: thread,
                    header: None,
                    last_stop: None,
                    turn: turn::TurnState::default(),
                },
            );
        }
        let list = if row.forgotten.is_some() {
            &mut forgotten
        } else {
            &mut matters
        };
        list.insert(
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
    let cached_limits = store.account_limits().await.unwrap_or_else(|error| {
        tracing::warn!(%error, "the stored account limits were not read");
        Vec::new()
    });
    let limits_reader = parts.limits_reader.unwrap_or_else(|| {
        super::limits::default_reader(&parts.effective, &parts.launcher, &parts.clock)
    });
    let mut actor = Actor {
        spawner,
        post,
        store,
        pipeline: Pipeline::new(heads),
        feeder,
        snapshots,
        effective: parts.effective,
        launcher: parts.launcher,
        profile: parts.profile,
        managed_mcp: parts.managed_mcp,
        codex_hooks: parts.codex_hooks,
        pre_write_guard: parts.pre_write_guard,
        wiring: parts.wiring,
        counts: parts.counts,
        count_on_commit: BTreeMap::new(),
        header_tools: super::header_tools::HeaderTools::default(),
        run_sweep: super::turn::RunSweep::default(),
        refused_folders: parts.refused_folders,
        home_spellings: super::rail::home_spellings(parts.home_folder.as_deref()),
        home: parts.home_folder,
        clock: parts.clock,
        ids: Ids::new(parts.ids),
        window_cap: parts.patch_window_bytes,
        app_capacity: parts.app_capacity,
        encoder: Encoder::new(),
        matters,
        forgotten,
        tails,
        threads,
        streams: BTreeMap::new(),
        event_subs: BTreeMap::new(),
        app_subs: Vec::new(),
        limits: super::limits::LimitsHost::new(limits_reader),
        lists: super::app_lists::Lists::default(),
        commands: BTreeMap::new(),
        folders: BTreeMap::new(),
        claims: BTreeMap::new(),
        folder_states: BTreeMap::new(),
        scans: super::scan::Scans::default(),
        scan_walk: parts.scan_walk,
        stats_running: std::sync::Arc::default(),
        files: super::papers::FileRefs::default(),
        next_sub: 0,
        work: Work::default(),
        phase: HostPhase::Starting(ready),
        scratch: None,
        #[cfg(test)]
        fault: None,
    };
    // The limits model and its first reads; nothing waits on them.
    super::limits::start(&mut actor, cached_limits);
    Ok(actor)
}

/// Queues the start-up work: the Scratch matter when there is none, and the load and
/// reconciliation of every thread that is not archived and whose matter is on the list (a
/// forgotten matter's threads were idle when it was forgotten, and stay as they are). The loop
/// answers `start` once all of it is done.
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
        .filter(|thread| !thread.row.archived && actor.matters.contains_key(&thread.row.matter))
        .map(|thread| thread.row.id)
        .collect();
    for &thread in &open {
        turn::expect_reconcile(actor, thread);
    }
    for thread in open {
        let stream = StreamId::Thread(thread);
        actor.with_loaded(
            stream,
            Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
                match loaded {
                    Ok(()) => turn::reconcile(actor, thread),
                    Err(error) => {
                        tracing::warn!(%error, %thread, "could not load a thread at start-up");
                        turn::reconcile_skipped(actor, thread);
                    }
                }
                actor.release(stream);
            }),
        );
    }
}
