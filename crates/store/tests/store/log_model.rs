//! `read_after` against a model: random commits and reads in four streams, each read checked
//! against the events the model kept. Reads go mostly to streams with events, with cursors and
//! limits at the edges a page can be wrong at: no cursor, the middle, the last seq and one either
//! side of it, and a limit that ends one short of, at or one past the head.

use clauseo_protocol::Envelope;
use clauseo_store::Store;
use futures::executor::block_on;
use proptest::prelude::*;
use proptest::test_runner::{RngSeed, TestRunner};

use crate::common::{TestDir, assert_seq_gap, seq, texts, thread};

/// Where a read's cursor sits in its stream, resolved when the read runs.
#[derive(Debug, Clone, Copy)]
enum Cursor {
    /// No cursor: from the first event.
    Start,
    /// After this seq, whatever the stream holds.
    Seq(u64),
    /// After half the stream (at least seq 1).
    Middle,
    /// After the head plus this (-1, 0 or 1), at least seq 1.
    Head(i8),
}

/// How many events a read asks for, resolved when the read runs.
#[derive(Debug, Clone, Copy)]
enum Limit {
    Fixed(u32),
    /// The events left after the cursor plus this (-1, 0 or 1), at least 0.
    ToHead(i8),
}

/// One step of [`read_after_matches_model`].
#[derive(Debug, Clone)]
enum Op {
    /// `count` events in stream `stream`, starting one past the head, or two past with `gap`.
    Commit {
        stream: usize,
        count: u64,
        gap: bool,
    },
    /// `read_after` in stream `pick`; with `filled`, `pick` chooses among the streams that have
    /// events instead (stream 3, which never has any, when none has).
    Read {
        filled: bool,
        pick: usize,
        cursor: Cursor,
        limit: Limit,
    },
}

fn op() -> impl Strategy<Value = Op> {
    let cursor = prop_oneof![
        2 => Just(Cursor::Start),
        2 => (1..24_u64).prop_map(Cursor::Seq),
        2 => Just(Cursor::Middle),
        3 => (-1..=1_i8).prop_map(Cursor::Head),
    ];
    let limit = prop_oneof![
        3 => (0..13_u32).prop_map(Limit::Fixed),
        2 => (-1..=1_i8).prop_map(Limit::ToHead),
    ];
    // 85 % of reads ask for a stream with events. They get one once the case has committed; reads
    // before that see only empty streams (151 of the seeded run's 236 reads hit events).
    prop_oneof![
        1 => (0..3_usize, 1..10_u64, prop::bool::weighted(0.15))
            .prop_map(|(stream, count, gap)| Op::Commit { stream, count, gap }),
        2 => (prop::bool::weighted(0.85), 0..4_usize, cursor, limit).prop_map(
            |(filled, pick, cursor, limit)| Op::Read {
                filled,
                pick,
                cursor,
                limit,
            }
        ),
    ]
}

/// `base + offset`, at least 1, as a seq number.
fn at_least_one(base: usize, offset: i8) -> u64 {
    let n = i64::try_from(base).unwrap() + i64::from(offset);
    u64::try_from(n.max(1)).unwrap()
}

/// Runs `ops` against `store`, op stream `n` as `thread(base + n)`, which no earlier case used.
fn run_model(store: &Store, base: u64, ops: &[Op]) {
    let thread = |stream: usize| thread(base + u64::try_from(stream).unwrap());
    let mut model: Vec<Vec<Envelope>> = vec![Vec::new(); 4];
    for op in ops {
        match *op {
            Op::Commit { stream, count, gap } => {
                let next = u64::try_from(model[stream].len()).unwrap() + 1;
                let start = if gap { next + 1 } else { next };
                let commit = texts(thread(stream), start, start + count - 1);
                let result = block_on(store.commit(commit.clone()));
                if gap {
                    assert_seq_gap(result, thread(stream), next, start);
                } else {
                    result.unwrap();
                    model[stream].extend(commit.events);
                }
            }
            Op::Read {
                filled,
                pick,
                cursor,
                limit,
            } => {
                let with_events: Vec<usize> = (0..4).filter(|&s| !model[s].is_empty()).collect();
                let stream = match (filled, with_events.len()) {
                    (false, _) => pick,
                    (true, 0) => 3,
                    (true, n) => with_events[pick % n],
                };
                let events = &model[stream];
                let after = match cursor {
                    Cursor::Start => None,
                    Cursor::Seq(n) => Some(n),
                    Cursor::Middle => Some(at_least_one(events.len() / 2, 0)),
                    Cursor::Head(offset) => Some(at_least_one(events.len(), offset)),
                };
                let from = after
                    .map_or(0, |after| usize::try_from(after).unwrap())
                    .min(events.len());
                let limit = match limit {
                    Limit::Fixed(n) => n,
                    Limit::ToHead(offset) => {
                        let left = i64::try_from(events.len() - from).unwrap();
                        u32::try_from((left + i64::from(offset)).max(0)).unwrap()
                    }
                };
                let to = (from + usize::try_from(limit).unwrap()).min(events.len());
                let got = block_on(store.read_after(thread(stream), after.map(seq), limit));
                assert_eq!(
                    got.unwrap(),
                    events[from..to],
                    "{op:?}: stream {stream} of {} events, after {after:?}, limit {limit}",
                    events.len()
                );
            }
        }
    }
}

#[test]
fn read_after_matches_model() {
    // Few cases as each commit syncs, a fixed seed, nothing beside the source, one store for all.
    let dir = TestDir::new("read_after_matches_model");
    let store = dir.open();
    let next = std::cell::Cell::new(0);
    TestRunner::new(ProptestConfig {
        cases: 32,
        rng_seed: RngSeed::Fixed(0x0051_07e7),
        failure_persistence: None,
        ..ProptestConfig::default()
    })
    .run(&prop::collection::vec(op(), 1..20), |ops| {
        run_model(&store, next.replace(next.get() + 4), &ops);
        Ok(())
    })
    .unwrap();
    block_on(store.close()).unwrap();
}
