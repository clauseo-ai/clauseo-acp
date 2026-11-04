//! store: the store's reads of stored bytes, through its public API.
//!
//! Items driven: `Store::open`, `Store::commit`, `Store::put_account_limits` and `Store::reopen`
//! (setup, then the reads under test), `Store::read_after`, `Store::read_blob`, `Store::receipt`,
//! `Store::account_limits`, `Store::matters` and `Store::threads`. A decoded snapshot goes through
//! `clauseo_reducer::Snapshot::from_bytes`, `into_state` and `apply`, as the host folds it.
//!
//! Setup: one store per process, in a scratch folder under the temp dir (see scratch.rs). It is
//! filled through `Store::commit` with the protocol v1 corpus events (renumbered per stream), a
//! snapshot at sequence 16 built by the reducer, a blob, a receipt, account limits, a matter row
//! and a thread row. Each iteration overwrites one stored value through a second, raw SQLite
//! connection (WAL mode, so the store's reader sees the write), reads it back through the store,
//! then restores the original.
//!
//! Input: byte 0 = selector (taken mod 6), byte 1 = flag, rest = payload.
//!   0: snapshots.state of the thread's snapshot (flag & 1: the payload is zstd-compressed first,
//!      so it passes decompression) -> `reopen`, then the snapshot decode and the tail fold.
//!   1: events.envelope of sequence 1 + flag % 8 -> `read_after` and `reopen`.
//!   2: blobs.data (flag & 1: size_bytes is also set to the payload's length) -> `read_blob` at
//!      offset flag >> 1, whole and ranged.
//!   3: receipts.result (lossy UTF-8) -> `receipt`.
//!   4: account_limits.limits (lossy UTF-8) -> `account_limits`.
//!   5: matters.name, kind and folder_path, and threads.title, acp_session_id (flag & 1) and
//!      agent_id; the payload is split on 0xff -> `matters` and `threads`.
//!
//! Invariants: reopen's snapshot sequence is at most the head; the tail holds only the asked
//! stream and is contiguous after the snapshot; `read_after` returns events in order from the
//! asked sequence; a compressed snapshot comes back exactly as the payload; a whole blob read
//! that is not empty and is under the read limit returns the original blob bytes (the store
//! checks the hash); a ranged read returns at most its length; a receipt that reads re-encodes.
#![no_main]
#![expect(
    clippy::disallowed_methods,
    reason = "fuzz harness, not app code: fuzz_target! and its scratch folder write with std::fs"
)]

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use clauseo_protocol::{
    AccountLimits, AgentId, BlobHash, CommandId, CommandOutcome, Envelope, MAX_BLOB_READ_BYTES,
    MatterId, MatterKind, RenameSource, Seq, StreamId, Timestamp,
};
use clauseo_reducer::{REDUCER_SCHEMA_VERSION, Snapshot as ReducerSnapshot, StreamState, apply};
use clauseo_store::{
    BlobIn, Commit, DB_FILE, MatterRow, Receipt, SnapshotIn, Store, StoreOptions, ThreadRow,
};
use futures::executor::block_on;
use libfuzzer_sys::fuzz_target;
use rusqlite::{Connection, params};

mod scratch;

static EVENTS: &str = include_str!("../../crates/protocol/tests/corpus/v1/events.jsonl");
const BLOB: &[u8] =
    b"The Seller shall deliver the Goods on the Completion Date. \xe2\x80\x94 clause 4.1";
const LIMITS: &str = r#"{"agent":"claude","account":"acct-1","billing":"subscription","windows":[],"at_limit":"stop","fetched_at":1760000000000}"#;

struct Fixture {
    _dir: PathBuf,
    store: Store,
    raw: Mutex<Connection>,
    thread: StreamId,
    matter: MatterId,
    command: CommandId,
    blob: BlobHash,
    snapshot_state: Vec<u8>,
    originals: Originals,
}

struct Originals {
    snapshot: Vec<u8>,
    envelopes: Vec<Vec<u8>>,
    receipt: String,
    limits: Option<String>,
    matter: (String, String, String),
    thread: (Option<String>, Option<String>, String),
}

/// The fixture's clock: 1.8e12 ms plus `n` ms.
fn fixture_time(n: u64) -> Timestamp {
    Timestamp::new(1_800_000_000_000 + i64::try_from(n).expect("a small n")).expect("a valid time")
}

fn fixture() -> &'static Fixture {
    static CELL: OnceLock<Fixture> = OnceLock::new();
    CELL.get_or_init(|| {
        let dir = scratch::create("store");
        let store = block_on(Store::open(StoreOptions::new(&dir))).expect("store opens");

        let mut counts: std::collections::BTreeMap<String, u64> = Default::default();
        // The corpus numbers seq across both streams; renumber per stream.
        let events: Vec<Envelope> = EVENTS
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let e: Envelope = serde_json::from_str(l).unwrap();
                let n = counts.entry(e.stream.to_string()).or_default();
                *n += 1;
                Envelope::new(
                    e.event_id,
                    e.stream,
                    Seq::new(*n).unwrap(),
                    e.ts,
                    e.command_id,
                    e.event,
                )
            })
            .collect();
        let thread = events
            .iter()
            .find(|e| matches!(e.stream, StreamId::Thread(_)))
            .unwrap()
            .stream;
        let matter_stream = events
            .iter()
            .find(|e| matches!(e.stream, StreamId::Matter(_)))
            .unwrap()
            .stream;
        let StreamId::Matter(matter) = matter_stream else {
            unreachable!()
        };
        let StreamId::Thread(thread_id) = thread else {
            unreachable!()
        };

        let matter_row = MatterRow {
            id: matter,
            name: "Fixture matter".to_owned(),
            kind: MatterKind::Folder,
            folder_path: "/clauseo-fixture/matter".to_owned(),
            folder_locator: None,
            shadow_repo_path: "/clauseo-fixture/shadow".to_owned(),
            created: fixture_time(0),
            client: None,
            next_date: None,
            pinned: false,
            last_opened: None,
            forgotten: None,
        };
        let thread_row = ThreadRow {
            id: thread_id,
            matter,
            agent: AgentId::new("claude-acp").expect("a valid agent id"),
            title: Some("Fixture thread".to_owned()),
            title_source: Some(RenameSource::Lawyer),
            acp_session_id: None,
            archived: false,
            created: fixture_time(0),
            updated: fixture_time(events.len() as u64),
            seen_seq: None,
        };
        let command: CommandId = "01900000-0000-7000-8000-0000000000aa".parse().unwrap();
        let blob = BlobIn::new(BLOB.to_vec());
        let blob_hash = blob.hash();

        // A snapshot after the first 16 thread events, built by the reducer itself.
        let thread_events: Vec<&Envelope> = events.iter().filter(|e| e.stream == thread).collect();
        let mut state = StreamState::new(thread);
        for event in thread_events.iter().take(16) {
            apply(&mut state, event).unwrap();
        }
        let snapshot_state = ReducerSnapshot::of(&state).to_bytes().unwrap();
        let snapshot = SnapshotIn::compress(
            thread,
            Seq::new(16).unwrap(),
            REDUCER_SCHEMA_VERSION,
            &snapshot_state,
        );

        block_on(store.commit(Commit {
            events: events.clone(),
            blobs: vec![blob],
            receipt: Some(Receipt {
                command_id: command,
                stream: Some(thread),
                payload_hash: [7; 32],
                result: CommandOutcome::Done,
                ts: Timestamp::new(1_760_000_000_000).unwrap(),
            }),
            thread: Some(thread_row),
            matter: Some(matter_row),
            checkpoints: Vec::new(),
            snapshot: Some(snapshot),
        }))
        .expect("fixture commit");
        if let Ok(limits) = serde_json::from_str::<AccountLimits>(LIMITS) {
            block_on(store.put_account_limits(limits)).expect("limits");
        }

        let raw = Connection::open(dir.join(DB_FILE)).unwrap();
        raw.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
        raw.execute_batch("PRAGMA synchronous = OFF").unwrap();
        let snapshot: Vec<u8> = raw
            .query_row("SELECT state FROM snapshots", [], |r| r.get(0))
            .unwrap();
        let envelopes = (1..=8)
            .map(|seq| {
                raw.query_row(
                    "SELECT envelope FROM events_readable WHERE stream = ?1 AND seq = ?2",
                    params![thread.to_string(), seq],
                    |r| r.get(0),
                )
                .unwrap()
            })
            .collect();
        let receipt: String = raw
            .query_row("SELECT result FROM receipts", [], |r| r.get(0))
            .unwrap();
        let limits: Option<String> = raw
            .query_row("SELECT limits FROM account_limits", [], |r| r.get(0))
            .ok();
        let matter_text = raw
            .query_row("SELECT name, kind, folder_path FROM matters", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        let thread_text = raw
            .query_row(
                "SELECT title, acp_session_id, agent_id FROM threads",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        Fixture {
            _dir: dir,
            store,
            raw: Mutex::new(raw),
            thread,
            matter,
            command,
            blob: blob_hash,
            snapshot_state,
            originals: Originals {
                snapshot,
                envelopes,
                receipt,
                limits,
                matter: matter_text,
                thread: thread_text,
            },
        }
    })
}

fn check_reopen(fx: &Fixture) {
    if let Ok(reopen) = block_on(fx.store.reopen(fx.thread, REDUCER_SCHEMA_VERSION)) {
        let head = reopen.head.map_or(0, |s| s.get());
        let from = reopen.snapshot.as_ref().map_or(0, |s| s.seq.get());
        assert!(from <= head, "snapshot at {from} beyond head {head}");
        for (i, envelope) in reopen.tail.iter().enumerate() {
            assert_eq!(
                envelope.stream, fx.thread,
                "tail holds another stream's event"
            );
            assert_eq!(
                envelope.seq.get(),
                from + 1 + i as u64,
                "tail is not contiguous"
            );
        }
        if let Some(snapshot) = reopen.snapshot {
            assert_eq!(snapshot.reducer_schema, REDUCER_SCHEMA_VERSION);
            if let Ok(snap) = ReducerSnapshot::from_bytes(&snapshot.state)
                && let Ok(mut state) = snap.into_state()
            {
                // The host folds the tail onto the snapshot's state.
                for envelope in &reopen.tail {
                    let _ = apply(&mut state, envelope);
                }
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let (selector, flag, payload) = (data[0], data[1], &data[2..]);
    let fx = fixture();
    let raw = fx.raw.lock().unwrap();
    let o = &fx.originals;
    match selector % 6 {
        0 => {
            let stored = if flag & 1 == 1 {
                zstd::bulk::compress(payload, 1).unwrap()
            } else {
                payload.to_vec()
            };
            raw.execute("UPDATE snapshots SET state = ?1", params![stored])
                .unwrap();
            if let Ok(reopen) = block_on(fx.store.reopen(fx.thread, REDUCER_SCHEMA_VERSION))
                && flag & 1 == 1
            {
                // A frame we compressed decompresses to exactly the payload.
                let snapshot = reopen
                    .snapshot
                    .expect("a decompressible snapshot is returned");
                assert_eq!(snapshot.state, payload, "decompressed snapshot differs");
            }
            check_reopen(fx);
            raw.execute("UPDATE snapshots SET state = ?1", params![o.snapshot])
                .unwrap();
        }
        1 => {
            let index = usize::from(flag % 8);
            let seq = index as i64 + 1;
            let key_sql = "UPDATE events SET envelope = ?1 WHERE key = \
                 ((SELECT stream_id FROM streams WHERE stream = ?2) << 32) | ?3";
            raw.execute(key_sql, params![payload, fx.thread.to_string(), seq])
                .unwrap();
            let after = if seq > 1 {
                Seq::new(seq as u64 - 1).ok()
            } else {
                None
            };
            if let Ok(page) = block_on(fx.store.read_after(fx.thread, after, 4)) {
                for (i, envelope) in page.iter().enumerate() {
                    assert_eq!(envelope.stream, fx.thread);
                    assert_eq!(envelope.seq.get(), seq as u64 + i as u64);
                }
            }
            check_reopen(fx);
            raw.execute(
                key_sql,
                params![o.envelopes[index], fx.thread.to_string(), seq],
            )
            .unwrap();
        }
        2 => {
            if flag & 1 == 1 {
                raw.execute(
                    "UPDATE blobs SET data = ?1, size_bytes = ?2",
                    params![payload, payload.len() as i64],
                )
                .unwrap();
            } else {
                raw.execute("UPDATE blobs SET data = ?1", params![payload])
                    .unwrap();
            }
            let offset = u64::from(flag >> 1);
            if let Ok(whole) = block_on(fx.store.read_blob(fx.blob, 0, u32::MAX)) {
                assert!(whole.len() as u64 <= u64::from(MAX_BLOB_READ_BYTES));
                // Payloads stay below MAX_BLOB_READ_BYTES (max_len), so this is a whole read,
                // which checks the hash (blob.rs:66): only the original bytes can come back.
                // (An empty row returns empty before any check, so it is skipped.)
                if !whole.is_empty() && payload.len() < MAX_BLOB_READ_BYTES as usize {
                    assert_eq!(
                        whole, BLOB,
                        "a whole read returned bytes that are not the blob"
                    );
                }
            }
            if let Ok(part) = block_on(fx.store.read_blob(fx.blob, offset, 7)) {
                assert!(part.len() <= 7, "ranged read returned {} bytes", part.len());
            }
            raw.execute(
                "UPDATE blobs SET data = ?1, size_bytes = ?2",
                params![BLOB, BLOB.len() as i64],
            )
            .unwrap();
        }
        3 => {
            let text = String::from_utf8_lossy(payload);
            raw.execute("UPDATE receipts SET result = ?1", params![text.as_ref()])
                .unwrap();
            if let Ok(Some(receipt)) = block_on(fx.store.receipt(fx.command)) {
                assert_eq!(receipt.command_id, fx.command);
                serde_json::to_string(&receipt.result).expect("a read receipt re-encodes");
            }
            raw.execute("UPDATE receipts SET result = ?1", params![o.receipt])
                .unwrap();
        }
        4 => {
            if let Some(original) = &o.limits {
                let text = String::from_utf8_lossy(payload);
                raw.execute(
                    "UPDATE account_limits SET limits = ?1",
                    params![text.as_ref()],
                )
                .unwrap();
                let _ = block_on(fx.store.account_limits());
                raw.execute("UPDATE account_limits SET limits = ?1", params![original])
                    .unwrap();
            }
        }
        _ => {
            let mut parts = payload.split(|&b| b == 0xff).map(String::from_utf8_lossy);
            let mut next = || parts.next().map(|s| s.into_owned()).unwrap_or_default();
            let (name, kind, folder) = (next(), next(), next());
            let (title, session, agent) = (next(), next(), next());
            raw.execute(
                "UPDATE matters SET name = ?1, kind = ?2, folder_path = ?3",
                params![name, kind, folder],
            )
            .unwrap();
            raw.execute(
                "UPDATE threads SET title = ?1, acp_session_id = ?2, agent_id = ?3",
                params![
                    title,
                    if flag & 1 == 1 { Some(session) } else { None },
                    agent
                ],
            )
            .unwrap();
            let _ = block_on(fx.store.matters());
            let _ = block_on(fx.store.threads(fx.matter));
            raw.execute(
                "UPDATE matters SET name = ?1, kind = ?2, folder_path = ?3",
                params![o.matter.0, o.matter.1, o.matter.2],
            )
            .unwrap();
            raw.execute(
                "UPDATE threads SET title = ?1, acp_session_id = ?2, agent_id = ?3",
                params![o.thread.0, o.thread.1, o.thread.2],
            )
            .unwrap();
        }
    }
    let _ = &fx.snapshot_state;
});
