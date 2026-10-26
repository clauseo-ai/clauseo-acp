//! The log: commits, `read_after` pages, the commit rules (atomic, no seq gap, one receipt per
//! command), unknown kinds and newer fields read back, the key bounds and the debug view.

use clauseo_protocol::{
    CommandOutcome, DomainEvent, Envelope, ErrorCode, MatterKind, StreamId, ThreadId,
};
use clauseo_store::{BlobIn, Commit, Error, MatterRow, READ_PAGE_MAX, SnapshotIn, Store};
use futures::executor::block_on;
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

use crate::common::{
    TestDir, assert_seq_gap, count, id, matter, receipt, seq, seqs, sql, stored_envelope,
    text_event, texts, thread, ts, unknown_event, unknown_json,
};

fn read(store: &Store, stream: StreamId, after: Option<u64>, limit: u32) -> Vec<Envelope> {
    block_on(store.read_after(stream, after.map(seq), limit)).unwrap()
}

fn heads(store: &Store) -> Vec<(StreamId, u64)> {
    block_on(store.heads())
        .unwrap()
        .into_iter()
        .map(|(stream, head)| (stream, head.get()))
        .collect()
}

#[test]
fn commit_then_read_after_pages() {
    let dir = TestDir::new("commit_then_read_after_pages");
    let store = dir.open();
    let commit = texts(thread(1), 1, 1_200);
    let written = commit.events.clone();
    let committed = block_on(store.commit(commit)).unwrap();
    assert_eq!(committed.heads, vec![(thread(1), seq(1_200))]);

    let first = read(&store, thread(1), None, 500);
    assert_eq!(first, written[..500]);
    assert_eq!(
        seqs(&read(&store, thread(1), Some(500), 500)),
        (501..=1_000).collect::<Vec<_>>()
    );
    assert_eq!(
        seqs(&read(&store, thread(1), Some(1_000), 500)),
        (1_001..=1_200).collect::<Vec<_>>()
    );
    assert_eq!(seqs(&read(&store, thread(1), Some(7), 3)), vec![8, 9, 10]);
    // A larger limit is capped at 500.
    assert_eq!(READ_PAGE_MAX, 500);
    assert_eq!(
        seqs(&read(&store, thread(1), None, u32::MAX)),
        (1..=500).collect::<Vec<_>>()
    );
    assert_eq!(read(&store, thread(1), None, 0), Vec::<Envelope>::new());
    // Nothing after the head, nothing in a stream that has no events.
    assert_eq!(
        read(&store, thread(1), Some(1_200), 500),
        Vec::<Envelope>::new()
    );
    assert_eq!(
        read(&store, thread(1), Some(5_000), 500),
        Vec::<Envelope>::new()
    );
    assert_eq!(read(&store, thread(9), None, 500), Vec::<Envelope>::new());

    // The log and the heads survive a close and an open.
    block_on(store.close()).unwrap();
    let store = dir.open();
    assert_eq!(heads(&store), vec![(thread(1), 1_200)]);
    assert_eq!(read(&store, thread(1), Some(1_199), 500), written[1_199..]);
    block_on(store.close()).unwrap();
}

#[test]
fn commit_is_atomic() {
    let dir = TestDir::new("commit_is_atomic");
    let store = dir.open();
    let mut first = texts(thread(1), 1, 2);
    first.receipt = Some(receipt(1, Some(thread(1))));
    block_on(store.commit(first)).unwrap();

    let blob = BlobIn::new(b"a blob only the failed commits name".to_vec());
    let failing = |events: Vec<Envelope>, command: u64| Commit {
        events,
        blobs: vec![blob.clone()],
        receipt: Some(receipt(command, Some(thread(2)))),
        matter: Some(MatterRow {
            id: matter(7),
            name: "Lease".to_owned(),
            kind: MatterKind::Folder,
            folder_path: "/m".to_owned(),
            folder_locator: None,
            shadow_repo_path: "/s".to_owned(),
            created: ts(0),
        }),
        snapshot: Some(SnapshotIn::compress(thread(2), seq(1), 1, b"state")),
        ..Commit::default()
    };
    // Its second stream has a gap, after its first stream's events and a new stream's.
    let mut gap = texts(thread(1), 3, 4).events;
    gap.push(text_event(thread(2), 1, "new stream"));
    gap.push(text_event(thread(3), 2, "gap"));
    assert_seq_gap(block_on(store.commit(failing(gap, 2))), thread(3), 1, 2);
    // Its receipt repeats the first commit's command.
    let duplicate = failing(texts(thread(1), 3, 3).events, 1);
    let result = block_on(store.commit(duplicate));
    assert!(
        matches!(result, Err(Error::DuplicateCommand { .. })),
        "{result:?}"
    );

    assert_eq!(heads(&store), vec![(thread(1), 2)]);
    assert_eq!(seqs(&read(&store, thread(1), None, 500)), vec![1, 2]);
    assert_eq!(read(&store, thread(2), None, 500), Vec::<Envelope>::new());
    assert!(block_on(store.receipt(id(2))).unwrap().is_none());
    let missing = block_on(store.read_blob(blob.hash(), 0, 16));
    assert!(
        matches!(missing, Err(Error::BlobMissing { .. })),
        "{missing:?}"
    );
    assert_eq!(block_on(store.matters()).unwrap(), Vec::<MatterRow>::new());
    assert!(
        block_on(store.reopen(thread(2), 1))
            .unwrap()
            .snapshot
            .is_none()
    );
    block_on(store.close()).unwrap();

    let conn = sql(&dir);
    assert_eq!(count(&conn, "SELECT count(*) FROM events"), 2);
    assert_eq!(count(&conn, "SELECT count(*) FROM streams"), 1);
    assert_eq!(count(&conn, "SELECT last_seq FROM streams"), 2);
    assert_eq!(count(&conn, "SELECT count(*) FROM receipts"), 1);
    for table in ["blobs", "snapshots", "matters"] {
        assert_eq!(
            count(&conn, &format!("SELECT count(*) FROM {table}")),
            0,
            "{table}"
        );
    }
}

#[test]
fn seq_gap_on_commit_is_rejected() {
    let dir = TestDir::new("seq_gap_on_commit_is_rejected");
    let store = dir.open();
    block_on(store.commit(texts(thread(1), 1, 3))).unwrap();

    let gap = block_on(store.commit(texts(thread(1), 5, 5)));
    assert!(
        matches!(&gap, Err(error) if error.code() == ErrorCode::Internal && !error.retryable())
    );
    assert_seq_gap(gap, thread(1), 4, 5);
    assert_seq_gap(
        block_on(store.commit(texts(thread(1), 3, 3))),
        thread(1),
        4,
        3,
    );
    // A new stream starts at 1.
    assert_seq_gap(
        block_on(store.commit(texts(thread(2), 2, 2))),
        thread(2),
        1,
        2,
    );
    // A gap inside one commit.
    let mut inner = texts(thread(1), 4, 4);
    inner.events.push(text_event(thread(1), 6, "six"));
    assert_seq_gap(block_on(store.commit(inner)), thread(1), 5, 6);

    assert_eq!(heads(&store), vec![(thread(1), 3)]);
    block_on(store.commit(texts(thread(1), 4, 5))).unwrap();
    assert_eq!(
        seqs(&read(&store, thread(1), None, 500)),
        vec![1, 2, 3, 4, 5]
    );
    block_on(store.close()).unwrap();
}

#[test]
fn duplicate_command_returns_stored_receipt() {
    let dir = TestDir::new("duplicate_command_returns_stored_receipt");
    let store = dir.open();
    let stored = receipt(1, Some(thread(1)));
    let mut first = texts(thread(1), 1, 1);
    first.receipt = Some(stored.clone());
    block_on(store.commit(first)).unwrap();
    assert_eq!(
        block_on(store.receipt(id(1))).unwrap(),
        Some(stored.clone())
    );
    assert_eq!(block_on(store.receipt(id(2))).unwrap(), None);

    // The same command_id with another payload and outcome: the first receipt comes back.
    let mut again = texts(thread(1), 2, 2);
    again.receipt = Some(clauseo_store::Receipt {
        payload_hash: [9; 32],
        result: CommandOutcome::ThreadCreated {
            thread: id::<ThreadId>(4),
        },
        stream: None,
        ..stored
    });
    match block_on(store.commit(again)) {
        Err(error) => {
            assert_eq!(error.code(), ErrorCode::Internal);
            assert!(!error.retryable());
            let Error::DuplicateCommand { stored: found } = error else {
                panic!("expected DuplicateCommand, got {error:?}")
            };
            assert_eq!(*found, stored);
        }
        other => panic!("expected DuplicateCommand, got {other:?}"),
    }
    assert_eq!(heads(&store), vec![(thread(1), 1)]);
    assert_eq!(block_on(store.receipt(id(1))).unwrap(), Some(stored));
    block_on(store.close()).unwrap();
}

#[test]
fn unknown_kind_reads_as_unknown() {
    let dir = TestDir::new("unknown_kind_reads_as_unknown");
    let store = dir.open();
    let commit = Commit {
        events: vec![
            text_event(thread(1), 1, "known"),
            unknown_event(thread(1), 2),
        ],
        ..Commit::default()
    };
    block_on(store.commit(commit)).unwrap();
    let read = read(&store, thread(1), None, 500);
    assert!(matches!(read[0].event, DomainEvent::TextAppended(_)));
    assert!(
        matches!(read[1].event, DomainEvent::Unknown { .. }),
        "{:?}",
        read[1].event
    );
    assert_eq!(read[1].event.kind(), "future_kind");
    assert_eq!(read[1].event.is_matter_level(), None);
    block_on(store.close()).unwrap();

    let conn = sql(&dir);
    let kind: String = conn
        .query_row(
            "SELECT kind FROM events_readable WHERE seq = 2",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(kind, "future_kind");
}

#[test]
fn unknown_kind_round_trips_verbatim_through_store() {
    let dir = TestDir::new("unknown_kind_round_trips_verbatim_through_store");
    let store = dir.open();
    let from_json = unknown_event(thread(1), 1);
    let packed = rmp_serde::to_vec_named(&unknown_event(thread(1), 2)).unwrap();
    let from_msgpack: Envelope = rmp_serde::from_slice(&packed).unwrap();
    let events = vec![from_json, from_msgpack];
    block_on(store.commit(Commit {
        events: events.clone(),
        ..Commit::default()
    }))
    .unwrap();
    assert_eq!(read(&store, thread(1), None, 500), events);
    block_on(store.close()).unwrap();

    let conn = sql(&dir);
    for (n, written) in (1..).zip(&events) {
        let stored = stored_envelope(&conn, thread(1), n);
        assert_eq!(stored, rmp_serde::to_vec_named(written).unwrap(), "seq {n}");
        let decoded: Envelope = rmp_serde::from_slice(&stored).unwrap();
        assert_eq!(
            rmp_serde::to_vec_named(&decoded).unwrap(),
            stored,
            "seq {n}"
        );
        // The JSON a client gets holds the payload as it came in.
        let json = serde_json::to_string(&decoded).unwrap();
        assert_eq!(json, unknown_json(thread(1), n), "seq {n}");
    }
}

#[test]
fn newer_optional_field_reads_from_store() {
    let dir = TestDir::new("newer_optional_field_reads_from_store");
    let store = dir.open();
    let events = vec![
        text_event(thread(1), 1, "one"),
        text_event(thread(1), 2, "two"),
    ];
    block_on(store.commit(Commit {
        events: events.clone(),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();

    // Seq 1 gains a field this version does not know at the top and one in the payload; seq 2
    // becomes schema_version 2. The envelope is a map whose last entry is the event, and the
    // payload is the event's last map, so both new entries go at the end of the bytes.
    let conn = sql(&dir);
    let mut newer = stored_envelope(&conn, thread(1), 1);
    bump_fixmap(&mut newer, b"\xADtext_appended");
    newer.extend_from_slice(b"\xACfuture_field\x92\x01\x02");
    bump_fixmap(&mut newer, b"");
    newer.extend_from_slice(b"\xAAfuture_top\xA5later");
    let mut newest = stored_envelope(&conn, thread(1), 2);
    let version = position(&newest, b"\xAEschema_version") + 15;
    assert_eq!(newest[version], 1);
    newest[version] = 2;
    for (n, bytes) in [(1, newer), (2, newest)] {
        let key = (1_i64 << 32) | n;
        let changed = conn
            .execute(
                "UPDATE events SET envelope = ?1 WHERE key = ?2",
                rusqlite::params![bytes, key],
            )
            .unwrap();
        assert_eq!(changed, 1);
    }
    drop(conn);

    let store = dir.open();
    assert_eq!(read(&store, thread(1), None, 1), events[..1]);
    match block_on(store.read_after(thread(1), None, 500)) {
        Err(error @ Error::NewerSchema { found: 2, .. }) => {
            assert_eq!(error.code(), ErrorCode::StoreNewerVersion);
        }
        other => panic!("expected NewerSchema, got {other:?}"),
    }
    block_on(store.close()).unwrap();
}

#[test]
fn streams_interleaved_read_back_separately() {
    let dir = TestDir::new("streams_interleaved_read_back_separately");
    let store = dir.open();
    let in_matter = StreamId::Matter(matter(3));
    let mixed = Commit {
        events: vec![
            text_event(thread(2), 1, "b1"),
            text_event(thread(1), 1, "a1"),
            text_event(thread(2), 2, "b2"),
            text_event(in_matter, 1, "m1"),
            text_event(thread(1), 2, "a2"),
        ],
        ..Commit::default()
    };
    let committed = block_on(store.commit(mixed.clone())).unwrap();
    assert_eq!(
        committed.heads,
        vec![
            (thread(2), seq(2)),
            (thread(1), seq(2)),
            (in_matter, seq(1))
        ]
    );
    block_on(store.commit(texts(thread(1), 3, 3))).unwrap();
    // A gap in one stream of an interleaved commit fails it whole.
    let bad = Commit {
        events: vec![
            text_event(thread(1), 4, "a4"),
            text_event(thread(2), 4, "b4"),
        ],
        ..Commit::default()
    };
    assert_seq_gap(block_on(store.commit(bad)), thread(2), 3, 4);

    let pick = |stream| -> Vec<Envelope> {
        mixed
            .events
            .iter()
            .filter(|e| e.stream == stream)
            .cloned()
            .collect()
    };
    assert_eq!(read(&store, thread(2), None, 500), pick(thread(2)));
    assert_eq!(read(&store, in_matter, None, 500), pick(in_matter));
    assert_eq!(seqs(&read(&store, thread(1), None, 500)), vec![1, 2, 3]);
    assert_eq!(read(&store, thread(1), None, 2), pick(thread(1)));
    let mut expected = vec![(thread(1), 3), (thread(2), 2), (in_matter, 1)];
    expected.sort_unstable();
    assert_eq!(heads(&store), expected);
    block_on(store.close()).unwrap();
}

#[test]
fn seq_and_stream_id_bounds() {
    let dir = TestDir::new("seq_and_stream_id_bounds");
    block_on(dir.open().close()).unwrap();
    // thread 1 sits one below the last seq a key holds; thread 2 takes stream_id 2^31 - 2, so
    // one more stream fits.
    let conn = sql(&dir);
    for (stream_id, stream, last_seq) in [
        (1_i64, thread(1), (1_i64 << 32) - 2),
        ((1 << 31) - 2, thread(2), 0),
    ] {
        conn.execute(
            "INSERT INTO streams (stream_id, stream, last_seq) VALUES (?1, ?2, ?3)",
            rusqlite::params![stream_id, stream.to_string(), last_seq],
        )
        .unwrap();
    }
    drop(conn);

    let store = dir.open();
    let last = (1_u64 << 32) - 1;
    block_on(store.commit(texts(thread(1), last, last))).unwrap();
    assert_eq!(
        seqs(&read(&store, thread(1), Some(last - 1), 500)),
        vec![last]
    );
    let over = block_on(store.commit(texts(thread(1), last + 1, last + 1)));
    match over {
        Err(error @ Error::KeyOutOfRange { .. }) => {
            assert_eq!(error.code(), ErrorCode::Internal);
            assert!(matches!(error, Error::KeyOutOfRange { stream } if stream == thread(1)));
        }
        other => panic!("expected KeyOutOfRange, got {other:?}"),
    }
    // The last stream_id, 2^31 - 1, goes to thread 3; thread 4 finds none left.
    block_on(store.commit(texts(thread(3), 1, 1))).unwrap();
    let full = block_on(store.commit(texts(thread(4), 1, 1)));
    assert!(
        matches!(full, Err(Error::KeyOutOfRange { stream }) if stream == thread(4)),
        "{full:?}"
    );
    block_on(store.commit(texts(thread(2), 1, 1))).unwrap();
    assert_eq!(seqs(&read(&store, thread(3), None, 500)), vec![1]);
    let mut expected = vec![(thread(1), last), (thread(2), 1), (thread(3), 1)];
    expected.sort_unstable();
    assert_eq!(heads(&store), expected);
    block_on(store.close()).unwrap();

    let conn = sql(&dir);
    assert_eq!(
        count(&conn, "SELECT max(stream_id) FROM streams"),
        (1 << 31) - 1
    );
    assert_eq!(count(&conn, "SELECT count(*) FROM streams"), 3);
    assert_eq!(count(&conn, "SELECT count(*) FROM events"), 3);
}

#[test]
fn events_readable_view_shows_stream() {
    let dir = TestDir::new("events_readable_view_shows_stream");
    let store = dir.open();
    let in_matter = StreamId::Matter(matter(3));
    let mut commit = texts(thread(1), 1, 2);
    commit.events.push(text_event(in_matter, 1, "m1"));
    block_on(store.commit(commit)).unwrap();
    block_on(store.close()).unwrap();

    let conn = sql(&dir);
    let mut statement = conn
        .prepare("SELECT stream, seq, kind FROM events_readable ORDER BY stream, seq")
        .unwrap();
    let rows: Vec<(String, i64, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let text = "text_appended".to_owned();
    let matter_text = in_matter.to_string();
    let thread_text = thread(1).to_string();
    assert!(matter_text.starts_with("matter:") && thread_text.starts_with("thread:"));
    assert_eq!(
        rows,
        vec![
            (matter_text, 1, text.clone()),
            (thread_text.clone(), 1, text.clone()),
            (thread_text, 2, text)
        ]
    );
    let none = "SELECT count(*) FROM events_readable WHERE stream = 'thread:none'";
    assert_eq!(count(&conn, none), 0);
}

/// One step of [`read_after_matches_model`].
#[derive(Debug, Clone)]
enum Op {
    /// `count` events in stream `stream`, starting one past the head, or two past with `gap`.
    Commit { stream: u64, count: u64, gap: bool },
    /// `read_after(stream, after, limit)`; stream 3 never has events.
    Read {
        stream: u64,
        after: Option<u64>,
        limit: u32,
    },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0..3_u64, 1..6_u64, prop::bool::weighted(0.15))
            .prop_map(|(stream, count, gap)| Op::Commit { stream, count, gap }),
        (0..4_u64, prop::option::of(1..24_u64), 0..12_u32).prop_map(|(stream, after, limit)| {
            Op::Read {
                stream,
                after,
                limit,
            }
        }),
    ]
}

fn run_model(ops: &[Op]) {
    let dir = TestDir::new("read_after_matches_model");
    let store = dir.open();
    let mut model: Vec<Vec<Envelope>> = vec![Vec::new(); 4];
    for op in ops {
        match *op {
            Op::Commit { stream, count, gap } => {
                let index = usize::try_from(stream).unwrap();
                let next = u64::try_from(model[index].len()).unwrap() + 1;
                let start = if gap { next + 1 } else { next };
                let commit = texts(thread(stream), start, start + count - 1);
                let result = block_on(store.commit(commit.clone()));
                if gap {
                    assert_seq_gap(result, thread(stream), next, start);
                } else {
                    result.unwrap();
                    model[index].extend(commit.events);
                }
            }
            Op::Read {
                stream,
                after,
                limit,
            } => {
                let events = &model[usize::try_from(stream).unwrap()];
                let from = after
                    .map_or(0, |after| usize::try_from(after).unwrap())
                    .min(events.len());
                let to = (from + usize::try_from(limit).unwrap()).min(events.len());
                assert_eq!(
                    read(&store, thread(stream), after, limit),
                    events[from..to],
                    "{op:?}"
                );
            }
        }
    }
    block_on(store.close()).unwrap();
}

proptest! {
    // Each commit pays a real sync, so few cases; a fixed seed keeps the run the same each time,
    // and nothing is written beside the source.
    #![proptest_config(ProptestConfig {
        cases: 32,
        rng_seed: RngSeed::Fixed(0x0051_07e7),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]
    #[test]
    fn read_after_matches_model(ops in prop::collection::vec(op(), 1..16)) {
        run_model(&ops);
    }
}

/// Where `needle` first starts in `bytes`.
fn position(bytes: &[u8], needle: &[u8]) -> usize {
    bytes
        .windows(needle.len())
        .position(|window| window == needle)
        .unwrap()
}

/// Adds one to the `MessagePack` fixmap header right after `key` (the first byte for an empty
/// `key`), for an entry appended to that map.
fn bump_fixmap(bytes: &mut [u8], key: &[u8]) {
    let at = if key.is_empty() {
        0
    } else {
        position(bytes, key) + key.len()
    };
    assert!(
        (0x80..0x8F).contains(&bytes[at]),
        "not a fixmap: {:#x}",
        bytes[at]
    );
    bytes[at] += 1;
}
