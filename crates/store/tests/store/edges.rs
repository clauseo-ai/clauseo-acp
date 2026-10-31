//! Store behaviours at the edges of SQLite: account limits kept per agent, a write that waits
//! out another connection's lock, a snapshot whose frame claims too much, and the WAL `close`
//! truncates while another connection has the file open.

use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AtLimitBehaviour, Billing, LimitSource,
};
use clauseo_store::{Commit, DB_FILE, Error, SnapshotIn};
use futures::executor::block_on;

use crate::common::{TestDir, seq, sql, texts, thread};

fn limits(agent: AgentKind, account: &str) -> AccountLimits {
    AccountLimits {
        agent,
        account: AccountRef(account.to_owned()),
        plan: None,
        billing: Billing::Plan,
        windows: Vec::new(),
        at_limit: AtLimitBehaviour::Pauses,
        source: LimitSource::Loading,
    }
}

/// Account limits are kept per agent and account: the same account name under Claude and
/// under Codex is two accounts, and both read back, Claude's first.
#[test]
fn account_limits_of_two_agents_with_one_account_name_are_both_kept() {
    let dir = TestDir::new("account_limits_of_two_agents_with_one_account_name_are_both_kept");
    let store = dir.open();
    let claude = limits(AgentKind::Claude, "work");
    let codex = limits(AgentKind::Codex, "work");
    block_on(store.put_account_limits(codex.clone())).unwrap();
    block_on(store.put_account_limits(claude.clone())).unwrap();
    assert_eq!(
        block_on(store.account_limits()).unwrap(),
        vec![claude, codex]
    );
    block_on(store.close()).unwrap();
}

/// A commit that cannot take the write lock, held by another connection past the busy
/// timeout (5 s), fails with `Io` whose `IoError` says busy, carries SQLite's failure as its
/// source and shows it; once the lock is free the next commit goes through.
#[test]
fn commit_blocked_by_another_writer_is_a_busy_io_error() {
    let dir = TestDir::new("commit_blocked_by_another_writer_is_a_busy_io_error");
    let store = dir.open();
    let other = sql(&dir);
    other.execute_batch("BEGIN IMMEDIATE").unwrap();

    match block_on(store.commit(texts(thread(1), 1, 1))) {
        Err(Error::Io { source }) => {
            assert!(source.is_busy(), "{source}");
            let cause = std::error::Error::source(&source).expect("SQLite's failure");
            assert_eq!(source.to_string(), cause.to_string());
        }
        other => panic!("expected a busy Io error, got {other:?}"),
    }

    other.execute_batch("ROLLBACK").unwrap();
    block_on(store.commit(texts(thread(1), 1, 1))).unwrap();
    block_on(store.close()).unwrap();
}

/// A snapshot whose zstd frame claims more than the 1 GiB cap is skipped without trying to
/// decompress it, as one that does not decompress is: `reopen` folds from the log instead.
#[test]
fn snapshot_claiming_more_than_the_cap_is_skipped() {
    let dir = TestDir::new("snapshot_claiming_more_than_the_cap_is_skipped");
    let store = dir.open();
    block_on(store.commit(Commit {
        snapshot: Some(SnapshotIn::compress(thread(1), seq(3), 1, b"state")),
        ..texts(thread(1), 1, 3)
    }))
    .unwrap();
    block_on(store.close()).unwrap();

    // A frame header alone: magic, single segment with an 8-byte content size, then 2^62.
    let mut frame = vec![0x28, 0xB5, 0x2F, 0xFD, 0xE0];
    frame.extend_from_slice(&(1_u64 << 62).to_le_bytes());
    sql(&dir)
        .execute("UPDATE snapshots SET state = ?1", [frame])
        .unwrap();

    let store = dir.open();
    let reopen = block_on(store.reopen(thread(1), 1)).unwrap();
    assert!(reopen.snapshot.is_none());
    assert_eq!(reopen.tail.len(), 3);
    block_on(store.close()).unwrap();
}

/// `close` checkpoints and truncates the WAL itself, so the main file holds every commit even
/// while another connection keeps the file open (and SQLite's own checkpoint on the last
/// close does not run).
#[test]
fn close_truncates_the_wal_while_another_connection_is_open() {
    let dir = TestDir::new("close_truncates_the_wal_while_another_connection_is_open");
    let store = dir.open();
    block_on(store.commit(texts(thread(1), 1, 20))).unwrap();
    let other = sql(&dir);
    let _rows: i64 = other
        .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
        .unwrap();
    let wal = dir.path().join(format!("{DB_FILE}-wal"));
    assert!(
        std::fs::metadata(&wal).unwrap().len() > 0,
        "the commit is in the WAL"
    );

    block_on(store.close()).unwrap();
    assert_eq!(std::fs::metadata(&wal).map_or(0, |meta| meta.len()), 0);
    drop(other);
}
