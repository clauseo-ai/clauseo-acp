//! Start-up: open the store, load the registry, start the feeder and the snapshot thread and the
//! limits model (its first reads run on their own), then
//! (in the loop) create the Scratch matter on the first start and reconcile every thread.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, mpsc};

use clauseo_protocol::{AccountLimits, MatterId, MatterKind, StreamId, ThreadId};
use clauseo_store::{Store, StoreOptions};
use futures::executor::LocalSpawner;

use super::input::Input;
use super::{Actor, HostPhase, MatterEntry, ThreadEntry, Work, turn};
use crate::config::HostConfig;
use crate::encode::Encoder;
use crate::error::Error;
use crate::ids::Ids;
use crate::log_file::Cause;
use crate::matter_worker::matter_settings;
use crate::pipeline::{FeederTx, Pipeline, Post, spawn_feeder};
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
    let store = Store::open(StoreOptions::new(data_dir.clone())).await?;
    // The index opens (or rebuilds) on its own thread: start-up does not wait for it.
    let search = crate::search::Search::start(
        data_dir,
        store.clone(),
        Arc::clone(&parts.clock),
        &parts.search,
    )?;
    #[cfg(unix)]
    crate::browser::session::sweep_stale(parts.wiring.browser_socket_root.as_deref());
    let heads = store.heads().await?;
    let tails = store.tail_times().await?.into_iter().collect();
    let Registry {
        matters,
        forgotten,
        threads,
    } = registry(&store).await?;
    let (feeder, snapshots) = start_writers(&spawner, &store, &post)?;
    let cached_limits = store.account_limits().await.unwrap_or_else(|error| {
        tracing::warn!(error = %Cause(&error), "the stored account limits were not read");
        Vec::new()
    });
    let limits_reader = parts.limits_reader.unwrap_or_else(|| {
        super::limits::default_reader(&parts.effective, &parts.launcher, &parts.clock)
    });
    let browser_install = super::browser_install::BrowserInstall::for_wiring(&parts.wiring);
    let update = super::update::start(
        parts.update,
        parts.effective.locations().clone(),
        post.clone(),
    );
    let mut actor = Actor {
        spawner,
        post,
        store,
        pipeline: Pipeline::new(heads),
        feeder,
        snapshots,
        search,
        effective: parts.effective,
        lock_causes: Vec::new(),
        launcher: parts.launcher,
        profile: parts.profile,
        managed_mcp: parts.managed_mcp,
        codex_hooks: parts.codex_hooks,
        folder_check: parts.folder_check,
        pre_write_guard: parts.pre_write_guard,
        // Before `wiring` moves.
        connectors: super::connectors::ConnectorsOnActor::new(parts.wiring.connectors.clone()),
        wiring: parts.wiring,
        counts: parts.counts,
        update,
        count_on_commit: BTreeMap::new(),
        browser_install,
        header_tools: super::header_tools::HeaderTools::default(),
        pack_generations: BTreeMap::new(),
        turn_path: super::turn::wide::TurnPath::default(),
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
        triage: super::triage::Triage::new(),
        lists: super::app_lists::Lists::default(),
        commands: BTreeMap::new(),
        folders: BTreeMap::new(),
        claims: BTreeMap::new(),
        folder_states: BTreeMap::new(),
        folder_supports: BTreeMap::new(),
        scans: super::scan::Scans::default(),
        scan_walk: parts.scan_walk,
        stats_running: std::sync::Arc::default(),
        files: super::papers::FileRefs::default(),
        next_sub: 0,
        work: Work::default(),
        phase: HostPhase::Starting(ready),
        scratch: None,
        made: BTreeMap::new(),
        #[cfg(test)]
        fault: None,
        #[cfg(test)]
        lost_replies: 0,
        #[cfg(test)]
        failed_sends: 0,
    };
    start_services(&mut actor, cached_limits);
    Ok(actor)
}

/// The feeder that commits the pipeline's writes, and the snapshot thread: both post their
/// results back to the loop through `post`.
fn start_writers(
    spawner: &LocalSpawner,
    store: &Store,
    post: &async_channel::Sender<Input>,
) -> Result<(FeederTx, SnapshotWorker), Error> {
    let commits = post.clone();
    let feeder_post: Post = Rc::new(move |ticket, result| {
        let _gone = commits.try_send(Input::Committed { ticket, result });
    });
    let feeder = spawn_feeder(spawner, store.clone(), feeder_post)?;
    let snapshot_post = post.clone();
    let snapshots = SnapshotWorker::start(Box::new(move |snapshot| {
        let _gone = snapshot_post.try_send(Input::Snapshot(snapshot));
    }))?;
    Ok((feeder, snapshots))
}

/// Starts what `open` leaves running: the agents' hold bound, the limits model with its
/// `cached` limits, the browser install and the lock causes; then logs the start.
fn start_services(actor: &mut Actor, cached: Vec<AccountLimits>) {
    // `agent.keys_loading`'s bound starts with the host (`agent/launch_refusals.rs`).
    let now = actor.clock.now().mono;
    actor.wiring.sign_in.refused.start_load_bound(now);
    // The limits model and its first reads; nothing waits on them.
    super::limits::start(actor, cached);
    super::browser_install::start(actor);
    super::connectors::start(actor);
    super::settings::log_lock_causes(&mut actor.lock_causes, &actor.effective);
    tracing::info!(
        target: crate::log_file::LIFECYCLE,
        matters = actor.matters.len(),
        threads = actor.threads.len(),
        "host started"
    );
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
                        tracing::warn!(
                            error = %Cause(&error),
                            %thread,
                            "could not load a thread at start-up"
                        );
                        turn::reconcile_skipped(actor, thread);
                    }
                }
                actor.release(stream);
            }),
        );
    }
}

/// The store's matters (forgotten ones apart) and every thread, as the actor holds them.
struct Registry {
    matters: BTreeMap<MatterId, MatterEntry>,
    forgotten: BTreeMap<MatterId, MatterEntry>,
    threads: BTreeMap<ThreadId, ThreadEntry>,
}

/// Reads the [`Registry`] from the store.
async fn registry(store: &Store) -> Result<Registry, Error> {
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
                    came_back: turn::CameBackWatches::default(),
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
    Ok(Registry {
        matters,
        forgotten,
        threads,
    })
}
