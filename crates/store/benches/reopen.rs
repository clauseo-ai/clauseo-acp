//! `reopen_store_20k`: the store's share of reopening a 20,000-event thread: `Store::open` on
//! the fixture, `reopen` (the snapshot at seq 18,001 read and decompressed, the 1,999-event tail
//! read and decoded), `close`. Wall clock: it reports and never fails a pull request.

#![expect(
    clippy::unwrap_used,
    reason = "bench setup and timed calls: a failure ends the bench run with its error"
)]

#[path = "common/dir.rs"]
mod dir;

use clauseo_store::{Store, StoreOptions, fixtures};
use criterion::{Criterion, criterion_group, criterion_main};
use futures::executor::block_on;

/// The reducer schema the fixture's snapshot is written under.
const REDUCER_SCHEMA: u32 = 1;

/// 1 MiB of deterministic bytes, a stand-in for the reducer's snapshot until its shape is fixed.
fn stand_in_snapshot() -> Vec<u8> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    (0..1 << 20)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            b"abcdefghijklmnopqrstuvwxyz \"{}:,0123456789"[(state % 42) as usize]
        })
        .collect()
}

fn reopen(c: &mut Criterion) {
    let dir = dir::BenchDir::new("reopen").unwrap();
    let options = StoreOptions::new(dir.path());
    block_on(async {
        let store = Store::open(options.clone()).await.unwrap();
        for commit in fixtures::reopen_20k(|_| stand_in_snapshot(), REDUCER_SCHEMA) {
            store.commit(commit).await.unwrap();
        }
        store.close().await.unwrap();
    });
    c.bench_function("reopen_store_20k", |b| {
        b.iter(|| {
            block_on(async {
                let store = Store::open(options.clone()).await.unwrap();
                let reopen = store
                    .reopen(fixtures::stream(), REDUCER_SCHEMA)
                    .await
                    .unwrap();
                assert_eq!(reopen.tail.len(), 1_999);
                store.close().await.unwrap();
                reopen
            })
        });
    });
}

criterion_group!(benches, reopen);
criterion_main!(benches);
