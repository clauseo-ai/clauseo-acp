//! `reopen_20k_events`: the time from `Host::subscribe` to its first item on a host started over
//! a copy of the fixture thread's store: 20,000 events, the snapshot at seq 18,001 and the
//! 1,999-event tail after it. Setup, the copy, `Host::start` and the shutdown are untimed; the
//! first item must be the view's `Reset`. Wall clock: it reports and never fails a pull request.

#![expect(
    clippy::unwrap_used,
    reason = "bench setup and timed calls: a failure ends the bench run with its error"
)]

#[path = "common/dir.rs"]
mod dir;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clauseo_host::{EncodedEvent, Host, HostConfig, HostHandle};
use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{Envelope, HostEvent, Principal, ViewPatchBody};
use clauseo_reducer::{REDUCER_SCHEMA_VERSION, Snapshot, StreamState, apply};
use clauseo_settings::{Effective, Locations, SystemFiles};
use clauseo_store::{Store, StoreOptions, fixtures};
use criterion::{Criterion, criterion_group, criterion_main};
use futures::StreamExt;
use futures::executor::block_on;

/// Folds the events it is given into the thread's state and returns the state's snapshot.
fn snapshot_of(events: &[Envelope]) -> Vec<u8> {
    let mut state = StreamState::new(fixtures::stream());
    for envelope in events {
        apply(&mut state, envelope).unwrap();
    }
    Snapshot::of(&state).to_bytes().unwrap()
}

/// Writes the fixture thread into a fresh store in `dir`, then closes it.
fn build_fixture(dir: &Path) {
    let options = StoreOptions::new(dir);
    block_on(async {
        let store = Store::open(options).await.unwrap();
        for commit in fixtures::reopen_20k(snapshot_of, REDUCER_SCHEMA_VERSION) {
            store.commit(commit).await.unwrap();
        }
        store.close().await.unwrap();
    });
}

/// Subscribes to the fixture thread from its start and returns the time until the first item
/// arrives, with that item.
#[expect(clippy::disallowed_methods, reason = "the bench's timer")]
fn timed_first_item(handle: &HostHandle) -> (Duration, EncodedEvent) {
    let start = Instant::now();
    let first = block_on(async {
        let mut subscription = handle
            .subscribe(Principal::LocalDesktop, fixtures::stream(), None)
            .await
            .unwrap();
        subscription.next().await.unwrap()
    });
    (start.elapsed(), first)
}

/// Whether `first` is a `HostEvent::Patches` whose first patch is a `Reset`.
fn is_reset(first: &EncodedEvent) -> bool {
    match first.decode().unwrap() {
        HostEvent::Patches(batch) => batch
            .patches
            .first()
            .is_some_and(|patch| matches!(patch.body, ViewPatchBody::Reset { .. })),
        _ => false,
    }
}

fn reopen_20k_events(c: &mut Criterion) {
    let bench = dir::BenchDir::new("reopen-20k-events").unwrap();
    let fixture = bench.path().join("fixture");
    dir::make(&fixture).unwrap();
    build_fixture(&fixture);
    let mut run = 0_u64;
    c.bench_function("reopen_20k_events", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                run += 1;
                let data = bench.path().join("runs").join(run.to_string());
                dir::copy_store(&fixture, &data).unwrap();
                let effective = Effective::load(&Locations::under_root(&data), &SystemFiles, &[]);
                let launcher = PassthroughLauncher::new(SpawnOptions::new());
                let config = HostConfig::new(effective, Arc::new(launcher));
                let (handle, join) = Host::start(config).unwrap();
                let (elapsed, first) = timed_first_item(&handle);
                assert!(
                    is_reset(&first),
                    "the first item after subscribe is not a Reset"
                );
                block_on(handle.shutdown()).unwrap();
                join.join().unwrap();
                dir::remove(&data).unwrap();
                total += elapsed;
            }
            total
        });
    });
}

criterion_group!(benches, reopen_20k_events);
criterion_main!(benches);
