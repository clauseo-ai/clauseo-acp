//! Shared test helpers: each test's own folder, fixed ids and envelopes, and opening a store.

use std::path::{Path, PathBuf};

use clauseo_protocol::{
    CommandId, CommandOutcome, DomainEvent, Envelope, EventId, MatterId, MessageId, Seq, StreamId,
    TextAppended, TextKind, ThreadId, Timestamp, TurnId,
};
use clauseo_store::{Commit, Receipt, Store, StoreOptions};
use futures::executor::block_on;

use crate::write;

/// A fresh folder for one test, `CARGO_TARGET_TMPDIR/clauseo-store-<name>-<pid>`, removed on
/// drop. Under the target folder rather than `temp_dir()`, so the tests' syncs go to the disk
/// the build uses (D: on the GitHub Windows runner, whose temp folder is on C:; issue #83).
pub(crate) struct TestDir(PathBuf);

impl TestDir {
    pub(crate) fn new(name: &str) -> Self {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("clauseo-store-{name}-{}", std::process::id()));
        write::remove_dir_all(&path);
        write::create_dir_all(&path);
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// A subfolder, created.
    pub(crate) fn sub(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        write::create_dir_all(&path);
        path
    }

    /// Opens a store here.
    pub(crate) fn open(&self) -> Store {
        block_on(Store::open(StoreOptions::new(self.path()))).unwrap()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        write::remove_dir_all(&self.0);
    }
}

/// A fixed v7 id whose low bits are `n`.
pub(crate) fn id<T: std::str::FromStr>(n: u64) -> T
where
    T::Err: std::fmt::Debug,
{
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

pub(crate) fn thread(n: u64) -> StreamId {
    StreamId::Thread(id::<ThreadId>(n))
}

pub(crate) fn matter(n: u64) -> MatterId {
    id(n)
}

pub(crate) fn seq(n: u64) -> Seq {
    Seq::new(n).unwrap()
}

pub(crate) fn ts(n: i64) -> Timestamp {
    Timestamp::new(1_800_000_000_000 + n).unwrap()
}

/// A `TextAppended` of `text` at `n` in `stream`.
pub(crate) fn text_event(stream: StreamId, n: u64, text: &str) -> Envelope {
    let event: DomainEvent = TextAppended {
        turn: id::<TurnId>(500),
        message: id::<MessageId>(501),
        acp_message_id: None,
        kind: TextKind::Answer,
        text: text.to_owned(),
    }
    .into();
    Envelope::new(
        id::<EventId>(1_000_000 + n),
        stream,
        seq(n),
        ts(i64::try_from(n).unwrap()),
        None,
        event,
    )
}

/// A commit of `TextAppended` events `from..=to` in `stream`.
pub(crate) fn texts(stream: StreamId, from: u64, to: u64) -> Commit {
    Commit {
        events: (from..=to)
            .map(|n| text_event(stream, n, &format!("text {n}")))
            .collect(),
        ..Commit::default()
    }
}

/// A receipt for command `n`.
pub(crate) fn receipt(n: u64, stream: Option<StreamId>) -> Receipt {
    Receipt {
        command_id: id::<CommandId>(n),
        stream,
        payload_hash: [n.to_le_bytes()[0]; 32],
        result: CommandOutcome::Done,
        ts: ts(0),
    }
}

/// The seqs of `envelopes`.
pub(crate) fn seqs(envelopes: &[Envelope]) -> Vec<u64> {
    envelopes
        .iter()
        .map(|envelope| envelope.seq.get())
        .collect()
}

/// A read-write connection to the store file in `dir`, for a test to inspect or patch rows
/// while no `Store` has the file open.
pub(crate) fn sql(dir: &TestDir) -> rusqlite::Connection {
    rusqlite::Connection::open(dir.path().join(clauseo_store::DB_FILE)).unwrap()
}

/// The single integer `query` returns.
pub(crate) fn count(conn: &rusqlite::Connection, query: &str) -> i64 {
    conn.query_row(query, [], |row| row.get(0)).unwrap()
}

/// The JSON text of an envelope at `n` in `stream` whose event has the kind `future_kind`,
/// which this version does not know, with a payload of several value types. Built as text, never
/// through `serde_json::Value`, whose key order depends on `serde_json`'s `preserve_order`
/// feature (on in the workspace build); the envelope's fields come in `Envelope`'s own order.
pub(crate) fn unknown_json(stream: StreamId, n: u64) -> String {
    let known = serde_json::to_string(&text_event(stream, n, "")).unwrap();
    let event = known.find(",\"event\":").unwrap();
    format!(
        "{},\"event\":{{\"future_kind\":{{\"note\":\"from a newer Clauseo\",\"count\":7,\
         \"below\":-3,\"flags\":[true,false,null],\"nested\":{{\"list\":[1,\"two\",\
         {{\"three\":3}}]}}}}}}}}",
        &known[..event]
    )
}

/// The envelope of [`unknown_json`], decoded from its JSON as a reader of the wire would.
pub(crate) fn unknown_event(stream: StreamId, n: u64) -> Envelope {
    serde_json::from_str(&unknown_json(stream, n)).unwrap()
}

/// The stored `events.envelope` bytes of `seq` in `stream`, read through the
/// `events_readable` view.
pub(crate) fn stored_envelope(conn: &rusqlite::Connection, stream: StreamId, seq: u64) -> Vec<u8> {
    conn.query_row(
        "SELECT envelope FROM events_readable WHERE stream = ?1 AND seq = ?2",
        rusqlite::params![stream.to_string(), i64::try_from(seq).unwrap()],
        |row| row.get(0),
    )
    .unwrap()
}

/// Asserts `result` is `Error::SeqGap` for `stream` with `expected` and `got`.
pub(crate) fn assert_seq_gap(
    result: Result<clauseo_store::Committed, clauseo_store::Error>,
    stream: StreamId,
    expected: u64,
    got: u64,
) {
    match result {
        Err(clauseo_store::Error::SeqGap {
            stream: found,
            expected: found_expected,
            got: found_got,
        }) => assert_eq!(
            (found, found_expected.get(), found_got.get()),
            (stream, expected, got)
        ),
        other => panic!("expected SeqGap in {stream} ({expected}, {got}), got {other:?}"),
    }
}
