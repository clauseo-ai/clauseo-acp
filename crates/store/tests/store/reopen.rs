//! Reopen: the newest snapshot of the asked reducer schema that decompresses, and the events
//! after it; the 20k-event fixture as the deterministic guard of the reopen budget.

use clauseo_protocol::Envelope;
use clauseo_store::fixtures::{self, reopen_20k};
use clauseo_store::{Commit, SnapshotIn, Store};
use futures::executor::block_on;

use crate::common::{TestDir, count, seq, seqs, sql, texts, thread};

/// Bytes that compress well and differ per `n`.
fn state(n: u8, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| if i % 64 == 0 { n } else { b'x' })
        .collect()
}

/// A commit of events `from..=to` in thread 1 with a snapshot at `at` of `reducer_schema`.
fn with_snapshot(from: u64, to: u64, at: u64, reducer_schema: u32, bytes: &[u8]) -> Commit {
    Commit {
        snapshot: Some(SnapshotIn::compress(
            thread(1),
            seq(at),
            reducer_schema,
            bytes,
        )),
        ..texts(thread(1), from, to)
    }
}

fn reopen_seqs(store: &Store, reducer_schema: u32) -> (Option<u64>, Vec<u64>, Option<u64>) {
    let reopen = block_on(store.reopen(thread(1), reducer_schema)).unwrap();
    (
        reopen.snapshot.map(|snapshot| snapshot.seq.get()),
        seqs(&reopen.tail),
        reopen.head.map(clauseo_protocol::Seq::get),
    )
}

#[test]
fn snapshot_in_compresses_and_reopen_returns_bytes() {
    let dir = TestDir::new("snapshot_in_compresses_and_reopen_returns_bytes");
    let bytes = state(3, 64 * 1024);
    let snapshot = SnapshotIn::compress(thread(1), seq(3), 7, &bytes);
    assert_eq!((snapshot.stream(), snapshot.seq()), (thread(1), seq(3)));
    assert!(
        snapshot.compressed().len() < bytes.len() / 10,
        "{}",
        snapshot.compressed().len()
    );
    assert_eq!(zstd::decode_all(snapshot.compressed()).unwrap(), bytes);

    let store = dir.open();
    block_on(store.commit(Commit {
        snapshot: Some(snapshot),
        ..texts(thread(1), 1, 5)
    }))
    .unwrap();
    let reopen = block_on(store.reopen(thread(1), 7)).unwrap();
    let found = reopen.snapshot.unwrap();
    assert_eq!((found.seq, found.reducer_schema), (seq(3), 7));
    assert_eq!(found.state, bytes);
    assert_eq!(seqs(&reopen.tail), vec![4, 5]);
    assert_eq!(reopen.head, Some(seq(5)));

    // A commit with a snapshot and no events; one past the head is not used.
    let at_head = state(5, 1000);
    let only = Commit {
        snapshot: Some(SnapshotIn::compress(thread(1), seq(5), 7, &at_head)),
        ..Commit::default()
    };
    block_on(store.commit(only)).unwrap();
    let ahead = Commit {
        snapshot: Some(SnapshotIn::compress(thread(1), seq(9), 7, b"ahead")),
        ..Commit::default()
    };
    block_on(store.commit(ahead)).unwrap();
    let reopen = block_on(store.reopen(thread(1), 7)).unwrap();
    assert_eq!(
        reopen
            .snapshot
            .map(|snapshot| (snapshot.seq, snapshot.state)),
        Some((seq(5), at_head))
    );
    assert_eq!(reopen.tail, Vec::<Envelope>::new());
    assert_eq!(reopen.head, Some(seq(5)));
    block_on(store.close()).unwrap();
}

#[test]
fn reopen_reads_snapshot_and_tail_only() {
    let dir = TestDir::new("reopen_reads_snapshot_and_tail_only");
    let bytes = state(1, 256 * 1024);
    let mut folded = 0;
    let commits = reopen_20k(
        |events: &[Envelope]| {
            folded = events.len();
            assert_eq!(
                seqs(events),
                (1..=fixtures::SNAPSHOT_SEQ).collect::<Vec<_>>()
            );
            bytes.clone()
        },
        4,
    );
    assert_eq!(folded, 18_001);
    let store = dir.open();
    for commit in commits {
        block_on(store.commit(commit)).unwrap();
    }
    let stream = fixtures::stream();
    let reopen = block_on(store.reopen(stream, 4)).unwrap();
    let snapshot = reopen.snapshot.unwrap();
    assert_eq!((snapshot.seq, snapshot.reducer_schema), (seq(18_001), 4));
    assert_eq!(snapshot.state, bytes);
    assert_eq!(reopen.tail.len(), 1_999);
    assert_eq!(seqs(&reopen.tail), (18_002..=20_000).collect::<Vec<_>>());
    assert!(reopen.tail.iter().all(|envelope| envelope.stream == stream));
    assert_eq!(reopen.head, Some(seq(20_000)));
    // Another reducer schema gets no snapshot and the whole log.
    let whole = block_on(store.reopen(stream, 5)).unwrap();
    assert!(whole.snapshot.is_none());
    assert_eq!(whole.tail.len(), 20_000);

    // Three snapshot commits in another stream keep its two newest.
    for n in 1..=3 {
        let commit = Commit {
            snapshot: Some(SnapshotIn::compress(thread(2), seq(n), 4, b"s")),
            ..texts(thread(2), n, n)
        };
        block_on(store.commit(commit)).unwrap();
    }
    let reopen = block_on(store.reopen(thread(2), 4)).unwrap();
    assert_eq!(reopen.snapshot.map(|snapshot| snapshot.seq), Some(seq(3)));
    block_on(store.close()).unwrap();
    let conn = sql(&dir);
    let kept = "SELECT group_concat(seq) FROM (SELECT seq FROM snapshots WHERE stream_id = \
                (SELECT stream_id FROM streams WHERE stream = ?1) ORDER BY seq)";
    let found: String = conn
        .query_row(kept, [thread(2).to_string()], |row| row.get(0))
        .unwrap();
    assert_eq!(found, "2,3");
    assert_eq!(count(&conn, "SELECT count(*) FROM snapshots"), 3);
}

#[test]
fn snapshot_with_other_reducer_schema_is_ignored() {
    let dir = TestDir::new("snapshot_with_other_reducer_schema_is_ignored");
    let store = dir.open();
    block_on(store.commit(with_snapshot(1, 2, 2, 2, b"schema 2"))).unwrap();
    block_on(store.commit(with_snapshot(3, 5, 3, 1, b"schema 1"))).unwrap();
    assert_eq!(reopen_seqs(&store, 2), (Some(2), vec![3, 4, 5], Some(5)));
    assert_eq!(reopen_seqs(&store, 1), (Some(3), vec![4, 5], Some(5)));
    assert_eq!(reopen_seqs(&store, 9), (None, vec![1, 2, 3, 4, 5], Some(5)));
    let reopen = block_on(store.reopen(thread(1), 2)).unwrap();
    assert_eq!(reopen.snapshot.unwrap().state, b"schema 2");
    block_on(store.close()).unwrap();
}

#[test]
fn reopen_of_unknown_stream_is_empty() {
    let dir = TestDir::new("reopen_of_unknown_stream_is_empty");
    let store = dir.open();
    block_on(store.commit(with_snapshot(1, 2, 2, 1, b"other stream"))).unwrap();
    let reopen = block_on(store.reopen(thread(9), 1)).unwrap();
    assert!(reopen.snapshot.is_none());
    assert_eq!(reopen.tail, Vec::<Envelope>::new());
    assert_eq!(reopen.head, None);
    block_on(store.close()).unwrap();
}

#[test]
fn corrupt_snapshot_falls_back_to_older_then_log() {
    let dir = TestDir::new("corrupt_snapshot_falls_back_to_older_then_log");
    let store = dir.open();
    block_on(store.commit(with_snapshot(1, 2, 2, 1, &state(2, 4096)))).unwrap();
    block_on(store.commit(with_snapshot(3, 5, 4, 1, &state(4, 4096)))).unwrap();
    assert_eq!(reopen_seqs(&store, 1), (Some(4), vec![5], Some(5)));
    block_on(store.close()).unwrap();

    // Seq 4's frame cut in half: zstd's header still reads, the data does not.
    let conn = sql(&dir);
    let compressed: Vec<u8> = conn
        .query_row("SELECT state FROM snapshots WHERE seq = 4", [], |row| {
            row.get(0)
        })
        .unwrap();
    let half = compressed[..compressed.len() / 2].to_vec();
    conn.execute("UPDATE snapshots SET state = ?1 WHERE seq = 4", [half])
        .unwrap();
    drop(conn);
    let store = dir.open();
    let reopen = block_on(store.reopen(thread(1), 1)).unwrap();
    assert_eq!(
        reopen
            .snapshot
            .map(|snapshot| (snapshot.seq, snapshot.state)),
        Some((seq(2), state(2, 4096)))
    );
    assert_eq!(seqs(&reopen.tail), vec![3, 4, 5]);
    block_on(store.close()).unwrap();

    // Seq 2's bytes become garbage after zstd's magic number: no snapshot, the whole log.
    let conn = sql(&dir);
    let garbage = [
        0x28_u8, 0xB5, 0x2F, 0xFD, 0xFF, 0x00, 0x13, 0x37, 0xDE, 0xAD,
    ];
    conn.execute(
        "UPDATE snapshots SET state = ?1 WHERE seq = 2",
        [garbage.as_slice()],
    )
    .unwrap();
    drop(conn);
    let store = dir.open();
    assert_eq!(reopen_seqs(&store, 1), (None, vec![1, 2, 3, 4, 5], Some(5)));
    block_on(store.close()).unwrap();
}
