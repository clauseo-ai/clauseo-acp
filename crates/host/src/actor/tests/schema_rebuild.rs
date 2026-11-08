//! The cost and the safety of a `REDUCER_SCHEMA_VERSION` bump: a thread whose only snapshot was
//! written by the schema before this build's is rebuilt from its whole log when it is first
//! opened, and a snapshot write that fails after the rebuild loses nothing: the log is intact
//! and the next open rebuilds again.
//!
//! The thread is `clauseo_store::fixtures::reopen_20k`, the reopen benches' thread: 20,000
//! events (80 turns of 250, 196 texts of 400 bytes and 25 tool calls each), the heaviest single
//! stream the benches model. A rebuild is per stream and lazy (on first open), so one heavy
//! stream is the unit of the cost a lawyer waits for. The time is printed, never asserted.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use clauseo_protocol::{CommandId, CommandOutcome, Envelope, Seq, Timestamp};
use clauseo_reducer::{REDUCER_SCHEMA_VERSION, Snapshot, SnapshotRef, StreamState, apply};
use clauseo_settings::{Effective, Locations, SystemFiles};
use clauseo_store::{Receipt, Reopen, Store, StoreOptions, fixtures};
use futures::executor::LocalPool;
use uuid::Uuid;

use super::super::input::Input;
use super::super::{Actor, HostPhase, shutdown, startup};
use crate::clock::{Clock, SystemClock};
use crate::pipeline::tests::support::TempDir;
use crate::{HostConfig, ManualClock, SequentialIds};

/// The schema before this build's: what wrote the stored snapshot before the bump.
const OLD_SCHEMA: u32 = REDUCER_SCHEMA_VERSION - 1;

/// The command whose receipt the fixture stores, so a commit given it again is refused.
const STORED: u128 = 0x5ca1;

/// All 20,000 fixture events, in order (the fixture's first commits up to the head).
fn all_events(commits: &[clauseo_store::Commit]) -> Vec<Envelope> {
    commits
        .iter()
        .flat_map(|c| c.events.iter().cloned())
        .collect()
}

/// The state every event folds to, from nothing.
fn fold_all(events: &[Envelope]) -> StreamState {
    let mut state = StreamState::new(fixtures::stream());
    for envelope in events {
        apply(&mut state, envelope).unwrap();
    }
    state
}

/// Snapshot bytes of the fold of `events`, as the build before the bump wrote them.
fn old_snapshot(events: &[Envelope]) -> Vec<u8> {
    let state = fold_all(events);
    SnapshotRef {
        reducer_schema: OLD_SCHEMA,
        state: &state,
    }
    .to_bytes()
    .unwrap()
}

/// This build's snapshot bytes of `state`: the durable part, as a snapshot holds it.
fn bytes_of(state: &StreamState) -> Vec<u8> {
    Snapshot::of(state).to_bytes().unwrap()
}

/// An actor on `dir`, driven one input at a time on the test's thread, as the turn rig does.
struct Driver {
    actor: Actor,
    pool: LocalPool,
    internal: async_channel::Receiver<Input>,
}

impl Driver {
    fn open(dir: &TempDir, run: u16) -> Self {
        let root = dir.path();
        let effective = Effective::load(&Locations::under_home(root), &SystemFiles, &[]);
        let launcher = clauseo_process::PassthroughLauncher::new(
            clauseo_process::SpawnOptions::new().home(root.join("home")),
        );
        let config = HostConfig::new(effective, std::sync::Arc::new(launcher))
            .with_clock(std::sync::Arc::new(ManualClock::new(
                Timestamp::new(1_790_000_000_000).unwrap(),
            )))
            .with_ids(Box::new(SequentialIds::with_run(run)))
            .with_managed_mcp_path(root.join("managed-mcp.json"));
        let mut pool = LocalPool::new();
        let (post, internal) = async_channel::unbounded();
        let (ready, _ready) = std::sync::mpsc::channel();
        let first = crate::actor::protection::observe(config.sandbox());
        let open = startup::open((config, first), pool.spawner(), post, ready);
        let mut actor = pool.run_until(open).unwrap();
        startup::begin_startup(&mut actor);
        let mut driver = Self {
            actor,
            pool,
            internal,
        };
        driver.run_until("start-up", |actor| {
            matches!(actor.phase, HostPhase::Serving)
        });
        driver
    }

    fn next(&mut self) -> Input {
        let recv = self.internal.recv();
        self.pool
            .run_until(recv)
            .expect("the actor's channel is open")
    }

    fn run_until(&mut self, what: &str, mut done: impl FnMut(&mut Actor) -> bool) {
        for _ in 0..1_000_000 {
            let _exit = self.actor.settle();
            if done(&mut self.actor) {
                return;
            }
            let input = self.next();
            self.actor.on_input(input);
        }
        panic!("never reached {what}");
    }

    /// Opens the fixture thread and returns how long the load took and the state it held. The
    /// waiter reads the state: with no subscriber, the load releases the stream right after.
    fn load(&mut self) -> (Duration, Vec<u8>) {
        let stream = fixtures::stream();
        assert!(
            self.actor.loaded(stream).is_none(),
            "loaded before it was asked"
        );
        let held: Rc<RefCell<Option<Vec<u8>>>> = Rc::default();
        let into = Rc::clone(&held);
        let clock = SystemClock;
        let started = clock.now().mono;
        self.actor.with_loaded(
            stream,
            Box::new(move |actor, result| {
                result.unwrap();
                *into.borrow_mut() = Some(bytes_of(&actor.loaded(stream).unwrap().state));
            }),
        );
        self.run_until("the load", |_| held.borrow().is_some());
        let took = clock.now().mono.saturating_sub(started);
        let state = held.borrow_mut().take().unwrap();
        (took, state)
    }

    /// Handles inputs until no commit, load or snapshot is in flight.
    fn settle(&mut self) {
        self.run_until("no work in flight", |actor| {
            actor.work.commits == 0 && actor.work.loads == 0 && actor.work.snapshots == 0
        });
    }

    fn reopen(&mut self) -> Reopen {
        let store = self.actor.store.clone();
        self.pool
            .run_until(store.reopen(fixtures::stream(), REDUCER_SCHEMA_VERSION))
            .unwrap()
    }

    fn close(mut self) {
        shutdown::begin_shutdown(&mut self.actor, None);
        loop {
            if self.actor.settle().is_some() {
                break;
            }
            let input = self.next();
            self.actor.on_input(input);
        }
        let Self {
            actor, mut pool, ..
        } = self;
        pool.run_until(shutdown::close(actor)).unwrap();
    }
}

/// Writes the fixture thread with its seq-18,001 snapshot at [`OLD_SCHEMA`] and a receipt for
/// [`STORED`]; returns every event.
fn write_fixture(dir: &TempDir) -> Vec<Envelope> {
    let data = dir.path().join("data");
    crate::write::create_dir_all(&data).unwrap();
    let mut commits = fixtures::reopen_20k(old_snapshot, OLD_SCHEMA);
    let events = all_events(&commits);
    if let Some(first) = commits.first_mut() {
        first.receipt = Some(Receipt {
            command_id: CommandId::new(Uuid::from_u128(STORED)).unwrap(),
            stream: None,
            payload_hash: [1; 32],
            result: CommandOutcome::Done,
            ts: Timestamp::new(1_800_000_000_000).unwrap(),
        });
    }
    futures::executor::block_on(async {
        let store = Store::open(StoreOptions::new(&data)).await.unwrap();
        for commit in commits {
            store.commit(commit).await.unwrap();
        }
        let reopen = store
            .reopen(fixtures::stream(), REDUCER_SCHEMA_VERSION)
            .await
            .unwrap();
        assert!(
            reopen.snapshot.is_none(),
            "only an old-schema snapshot is stored"
        );
        assert_eq!(
            reopen.tail.len(),
            events.len(),
            "a reopen at this schema reads all"
        );
        store.close().await.unwrap();
    });
    events
}

#[test]
fn an_old_schema_snapshot_rebuilds_20k_events_and_a_failed_snapshot_write_loses_nothing() {
    let dir = TempDir::new("actor-schema-rebuild");
    let events = write_fixture(&dir);
    assert_eq!(events.len(), 20_000);
    let clock = SystemClock;
    let started = clock.now().mono;
    let expected = bytes_of(&fold_all(&events));
    let fold_took = clock.now().mono.saturating_sub(started);
    let head = Seq::new(fixtures::EVENTS).unwrap();

    // First open after the bump, with every commit of no events (the snapshot's) refused.
    let mut driver = Driver::open(&dir, 1);
    let refused = Rc::new(Cell::new(0_u32));
    let count = Rc::clone(&refused);
    driver.actor.fault = Some(Box::new(move |events: &[Envelope]| {
        events.is_empty().then(|| {
            count.set(count.get() + 1);
            CommandId::new(Uuid::from_u128(STORED)).unwrap()
        })
    }));
    let (first_took, first) = driver.load();
    assert!(
        first == expected,
        "the rebuilt state is not the fold of every event"
    );
    driver.settle();
    assert_eq!(
        refused.get(),
        1,
        "the rebuild's snapshot write was the one refused"
    );
    let after_failure = driver.reopen();
    assert!(
        after_failure.snapshot.is_none(),
        "no snapshot of this schema was stored"
    );
    assert_eq!(after_failure.tail, events, "the log is whole and unchanged");
    driver.close();

    // The next open rebuilds again, and this time stores the snapshot at the head.
    let mut driver = Driver::open(&dir, 2);
    let (second_took, second) = driver.load();
    assert!(
        second == expected,
        "the second rebuild is not the fold of every event"
    );
    driver.settle();
    let stored = driver.reopen();
    let snapshot = stored
        .snapshot
        .expect("a snapshot of this schema after the second load");
    assert_eq!(snapshot.seq, head);
    let state = Snapshot::from_bytes(&snapshot.state)
        .and_then(Snapshot::into_state)
        .unwrap();
    assert!(
        bytes_of(&state) == expected,
        "the stored snapshot is not the fold"
    );
    assert!(stored.tail.is_empty(), "nothing after the head");
    driver.close();

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    eprintln!(
        "schema rebuild of 20,000 events ({profile}): fold alone {fold_took:?}; first open \
         (store read + fold + view) {first_took:?}; second open after the failed snapshot \
         write {second_took:?}"
    );
}
