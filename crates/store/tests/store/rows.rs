//! The rows beside the log: blobs, account limits, matters, threads, checkpoints and receipts.

use clauseo_protocol::{
    AccountLimits, AccountRef, AgentId, AgentKind, AgentSessionId, AtLimitBehaviour, Billing,
    BlobHash, CheckpointId, CommandId, LimitSource, MAX_BLOB_READ_BYTES, MatterId, MatterKind,
    PlanLabel, ThreadId,
};
use clauseo_store::{
    BlobIn, CheckpointRow, Commit, DB_FILE, Error, MatterRow, ThreadRow, blob_ref,
};
use futures::executor::block_on;
use rusqlite::Connection;

use crate::common::{TestDir, id, matter, receipt, seq, seqs, text_event, thread, ts};

/// `len` bytes that differ from one position to the next.
fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|n| u8::try_from(n % 251).unwrap()).collect()
}

fn blob_rows(dir: &TestDir) -> i64 {
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    conn.query_row("SELECT count(*) FROM blobs", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn blob_round_trip_and_dedupe() {
    let dir = TestDir::new("blob_round_trip_and_dedupe");
    let store = dir.open();
    let bytes = b"a tool's output, stored once".to_vec();
    let blob = BlobIn::new(bytes.clone());
    assert_eq!(blob.blob_ref(), blob_ref(&bytes));
    let hash = blob.hash();
    block_on(store.commit(Commit {
        blobs: vec![blob.clone(), BlobIn::new(bytes.clone())],
        ..Commit::default()
    }))
    .unwrap();
    // The same bytes again in a later commit.
    block_on(store.commit(Commit {
        blobs: vec![blob],
        ..Commit::default()
    }))
    .unwrap();
    assert_eq!(block_on(store.read_blob(hash, 0, 1024)).unwrap(), bytes);

    let missing = BlobHash::from_bytes([7; 32]);
    let error = block_on(store.read_blob(missing, 0, 1024)).unwrap_err();
    assert!(
        matches!(error, Error::BlobMissing { hash } if hash == missing),
        "{error:?}"
    );
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::StoreCorrupt);
    block_on(store.close()).unwrap();
    assert_eq!(blob_rows(&dir), 1);
}

#[test]
fn blob_range_read() {
    let dir = TestDir::new("blob_range_read");
    let store = dir.open();
    let cap = MAX_BLOB_READ_BYTES as usize;
    let bytes = pattern(cap + 1000);
    let size = bytes.len() as u64;
    let blob = BlobIn::new(bytes.clone());
    let hash = blob.hash();
    block_on(store.commit(Commit {
        blobs: vec![blob],
        ..Commit::default()
    }))
    .unwrap();
    let read = |offset: u64, len: u32| block_on(store.read_blob(hash, offset, len)).unwrap();
    assert_eq!(read(10, 20), bytes[10..30]);
    // Running past the end stops at the end.
    assert_eq!(read(size - 5, 100), bytes[cap + 995..]);
    assert_eq!(read(size, 10), Vec::<u8>::new());
    assert_eq!(read(size + 10, 10), Vec::<u8>::new());
    // One read returns at most MAX_BLOB_READ_BYTES.
    assert_eq!(read(0, u32::MAX), bytes[..cap]);
    assert_eq!(read(1000, u32::MAX), bytes[1000..]);
    block_on(store.close()).unwrap();

    // Alter one stored byte, keeping the length.
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    let mut altered = bytes;
    altered[500] ^= 0xFF;
    conn.execute("UPDATE blobs SET data = ?1", [&altered])
        .unwrap();
    drop(conn);
    let store = dir.open();
    // No single read covers this blob whole (it is past the cap), so none checks its hash: a
    // part comes back altered. The whole-read check is shown with a short blob below.
    let ranged = block_on(store.read_blob(hash, 400, 200)).unwrap();
    assert_eq!(
        ranged,
        altered[400..600],
        "a ranged read does not check the hash"
    );
    block_on(store.close()).unwrap();

    let small = b"short enough to read whole".to_vec();
    let store = dir.open();
    let small_hash = BlobIn::new(small.clone()).hash();
    block_on(store.commit(Commit {
        blobs: vec![BlobIn::new(small.clone())],
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    let mut altered = small;
    altered[0] ^= 0xFF;
    conn.execute(
        "UPDATE blobs SET data = ?1 WHERE hash = ?2",
        rusqlite::params![altered, small_hash.as_bytes().as_slice()],
    )
    .unwrap();
    drop(conn);
    let store = dir.open();
    let error = block_on(store.read_blob(small_hash, 0, 1024)).unwrap_err();
    assert!(matches!(error, Error::Corrupt { .. }), "{error:?}");
    // A part of it is returned unchecked.
    assert_eq!(
        block_on(store.read_blob(small_hash, 1, 4)).unwrap(),
        altered[1..5]
    );
    block_on(store.close()).unwrap();
}

fn limits(agent: AgentKind, account: &str, plan: &str) -> AccountLimits {
    AccountLimits {
        agent,
        account: AccountRef(account.to_owned()),
        plan: Some(PlanLabel(plan.to_owned())),
        billing: Billing::Plan,
        windows: Vec::new(),
        at_limit: AtLimitBehaviour::Pauses,
        source: LimitSource::Loading,
    }
}

#[test]
fn account_limits_last_value_per_account() {
    let dir = TestDir::new("account_limits_last_value_per_account");
    let store = dir.open();
    let first = limits(AgentKind::Claude, "acct-a", "Pro");
    let other = limits(AgentKind::Codex, "acct-b", "Plus");
    let second = limits(AgentKind::Claude, "acct-a", "Max");
    block_on(store.put_account_limits(first)).unwrap();
    block_on(store.put_account_limits(other.clone())).unwrap();
    block_on(store.put_account_limits(second.clone())).unwrap();
    assert_eq!(
        block_on(store.account_limits()).unwrap(),
        vec![second.clone(), other.clone()]
    );
    block_on(store.close()).unwrap();

    // A stored value that does not decode is left out; the others still read.
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    conn.execute(
        "INSERT INTO account_limits (agent, account, limits) VALUES ('claude', 'acct-c', 'nope')",
        [],
    )
    .unwrap();
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM account_limits", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 3);
    drop(conn);
    let store = dir.open();
    assert_eq!(
        block_on(store.account_limits()).unwrap(),
        vec![second, other]
    );
    block_on(store.close()).unwrap();
}

fn matter_row(n: u64, created: i64) -> MatterRow {
    MatterRow {
        id: matter(n),
        name: format!("Matter {n}"),
        kind: MatterKind::Folder,
        folder_path: format!("/matters/{n}"),
        folder_locator: Some(vec![1, 2, 3]),
        shadow_repo_path: format!("/shadow/{n}"),
        created: ts(created),
    }
}

fn thread_row(n: u64, matter: MatterId, updated: i64) -> ThreadRow {
    ThreadRow {
        id: id::<ThreadId>(n),
        matter,
        agent: AgentId::new("claude-code").unwrap(),
        title: Some(format!("Thread {n}")),
        acp_session_id: Some(AgentSessionId::new(&format!("sess_{n}")).unwrap()),
        archived: n.is_multiple_of(2),
        created: ts(1),
        updated: ts(updated),
    }
}

#[test]
fn matter_and_thread_rows_round_trip() {
    let dir = TestDir::new("matter_and_thread_rows_round_trip");
    let store = dir.open();
    let first = matter_row(1, 1);
    let scratch = MatterRow {
        kind: MatterKind::Scratch,
        folder_locator: None,
        ..matter_row(2, 2)
    };
    let older = thread_row(1, matter(1), 5);
    let newer = thread_row(2, matter(1), 9);
    let untitled = ThreadRow {
        title: None,
        acp_session_id: None,
        ..thread_row(3, matter(2), 3)
    };
    block_on(store.commit(Commit {
        matter: Some(first.clone()),
        thread: Some(older.clone()),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.commit(Commit {
        matter: Some(scratch.clone()),
        thread: Some(newer.clone()),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.commit(Commit {
        thread: Some(untitled.clone()),
        ..Commit::default()
    }))
    .unwrap();

    // A thread naming a matter with no row fails its commit, events and all.
    let orphan = thread_row(4, matter(9), 1);
    let error = block_on(store.commit(Commit {
        events: vec![text_event(thread(4), 1, "orphan")],
        thread: Some(orphan),
        ..Commit::default()
    }))
    .unwrap_err();
    assert!(matches!(error, Error::Constraint { .. }), "{error:?}");
    assert_eq!(block_on(store.threads(matter(9))).unwrap(), Vec::new());
    let read = block_on(store.read_after(thread(4), None, 10)).unwrap();
    assert_eq!(seqs(&read), Vec::<u64>::new());
    block_on(store.close()).unwrap();

    let store = dir.open();
    assert_eq!(
        block_on(store.matters()).unwrap(),
        vec![first.clone(), scratch]
    );
    assert_eq!(
        block_on(store.threads(matter(1))).unwrap(),
        vec![newer.clone(), older]
    );
    assert_eq!(block_on(store.threads(matter(2))).unwrap(), vec![untitled]);
    assert_eq!(block_on(store.heads()).unwrap(), Vec::new());

    // A second put replaces the row.
    let renamed = MatterRow {
        name: "Renamed".to_owned(),
        ..first
    };
    let moved = thread_row(1, matter(1), 20);
    block_on(store.commit(Commit {
        matter: Some(renamed.clone()),
        thread: Some(moved.clone()),
        ..Commit::default()
    }))
    .unwrap();
    assert_eq!(block_on(store.matters()).unwrap()[0], renamed);
    assert_eq!(
        block_on(store.threads(matter(1))).unwrap(),
        vec![moved, newer]
    );
    block_on(store.close()).unwrap();
}

#[test]
fn checkpoint_rows_round_trip() {
    let dir = TestDir::new("checkpoint_rows_round_trip");
    let store = dir.open();
    let one = CheckpointRow {
        id: id::<CheckpointId>(1),
        matter: matter(1),
        commit_oid: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        seq: seq(4),
    };
    let two = CheckpointRow {
        id: id::<CheckpointId>(2),
        seq: seq(9),
        ..one.clone()
    };
    block_on(store.commit(Commit {
        matter: Some(matter_row(1, 1)),
        checkpoints: vec![one.clone(), two.clone()],
        ..Commit::default()
    }))
    .unwrap();

    // A checkpoint naming a matter with no row fails its commit.
    let orphan = CheckpointRow {
        id: id::<CheckpointId>(3),
        matter: matter(9),
        ..one.clone()
    };
    let error = block_on(store.commit(Commit {
        checkpoints: vec![orphan],
        ..Commit::default()
    }))
    .unwrap_err();
    assert!(matches!(error, Error::Constraint { .. }), "{error:?}");
    block_on(store.close()).unwrap();

    let store = dir.open();
    assert_eq!(block_on(store.checkpoint(one.id)).unwrap(), Some(one));
    assert_eq!(block_on(store.checkpoint(two.id)).unwrap(), Some(two));
    assert_eq!(
        block_on(store.checkpoint(id::<CheckpointId>(3))).unwrap(),
        None
    );
    block_on(store.close()).unwrap();
}

#[test]
fn receipts_round_trip_and_unknown_is_none() {
    let dir = TestDir::new("receipts_round_trip_and_unknown_is_none");
    let store = dir.open();
    let with_stream = receipt(1, Some(thread(1)));
    let without = receipt(2, None);
    block_on(store.commit(Commit {
        events: vec![text_event(thread(1), 1, "one")],
        receipt: Some(with_stream.clone()),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.commit(Commit {
        receipt: Some(without.clone()),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();

    let store = dir.open();
    let read = |n: u64| block_on(store.receipt(id::<CommandId>(n))).unwrap();
    assert_eq!(read(1), Some(with_stream));
    assert_eq!(read(2), Some(without));
    assert_eq!(read(3), None);
    let events = block_on(store.read_after(thread(1), None, 10)).unwrap();
    assert_eq!(seqs(&events), vec![1]);
    block_on(store.close()).unwrap();
}
