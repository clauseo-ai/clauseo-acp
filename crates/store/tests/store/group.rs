//! Group commit: commits queued while the writer is held form one group.
//!
//! Each test holds the writer thread with the test-support hook `Store::hold_writer`, queues its
//! commits with `send_commit` (which returns once a commit is in the queue; these tests never
//! fill its 256 slots), then releases it. The writer then takes every queued commit as one group,
//! each in its own savepoint. So the grouping is certain, not a matter of timing:
//! `group_commit_failure_rolls_back_only_its_savepoint` runs `ROLLBACK TO` and `RELEASE` inside a
//! shared transaction, and `pipelined_commits_of_one_stream_share_a_group` puts eight commits of
//! one stream in one group.

use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AtLimitBehaviour, Billing, Envelope, LimitSource,
    StreamId,
};
use clauseo_store::{BlobIn, Commit, CommitReply, Committed, Error, Store, StoreOptions};
use futures::executor::block_on;

use crate::common::{
    TestDir, assert_seq_gap, count, id, receipt, seq, seqs, sql, text_event, texts, thread,
};

fn send(store: &Store, commit: Commit) -> CommitReply {
    block_on(store.send_commit(commit)).unwrap()
}

/// Holds the writer, runs `queue` (which sends the group's commits), then releases it.
fn grouped<T>(store: &Store, queue: impl FnOnce() -> T) -> T {
    let release = block_on(store.hold_writer()).unwrap();
    let queued = queue();
    release.send(()).unwrap();
    queued
}

fn read(store: &Store, stream: StreamId) -> Vec<Envelope> {
    block_on(store.read_after(stream, None, 500)).unwrap()
}

#[test]
fn group_commit_each_caller_gets_own_result() {
    let dir = TestDir::new("group_commit_each_caller_gets_own_result");
    let store = dir.open();
    let replies: Vec<CommitReply> = grouped(&store, || {
        let mut replies: Vec<CommitReply> = (1..=4)
            .map(|n| send(&store, texts(thread(n), 1, n)))
            .collect();
        // A commit with a gap, between good ones.
        replies.insert(2, send(&store, texts(thread(5), 2, 2)));
        replies
    });
    let mut results: Vec<_> = replies.into_iter().map(block_on).collect();
    assert_seq_gap(results.remove(2), thread(5), 1, 2);
    for (n, result) in (1..).zip(results) {
        assert_eq!(result.unwrap().heads, vec![(thread(n), seq(n))]);
        assert_eq!(seqs(&read(&store, thread(n))), (1..=n).collect::<Vec<_>>());
    }
    assert_eq!(read(&store, thread(5)), Vec::<Envelope>::new());
    block_on(store.close()).unwrap();
}

#[test]
fn group_commit_failure_rolls_back_only_its_savepoint() {
    let dir = TestDir::new("group_commit_failure_rolls_back_only_its_savepoint");
    let store = dir.open();
    let blob = BlobIn::new(b"named only by the failed commit".to_vec());
    let c_commit = texts(thread(1), 3, 4);
    let release = block_on(store.hold_writer()).unwrap();
    let a = send(&store, texts(thread(1), 1, 2));
    // B writes thread 1's seq 3, a new stream and a blob before its gap in thread 3.
    let mut b = texts(thread(1), 3, 3);
    b.events.push(text_event(thread(2), 1, "b's new stream"));
    b.events.push(text_event(thread(3), 2, "gap"));
    b.blobs.push(blob.clone());
    b.receipt = Some(receipt(2, None));
    let b = send(&store, b);
    // C takes seq 3, which B never wrote; D takes a new stream after B's was dropped.
    let c = send(&store, c_commit.clone());
    let d = send(&store, texts(thread(4), 1, 1));
    release.send(()).unwrap();

    assert_eq!(block_on(a).unwrap().heads, vec![(thread(1), seq(2))]);
    assert_seq_gap(block_on(b), thread(3), 1, 2);
    assert_eq!(block_on(c).unwrap().heads, vec![(thread(1), seq(4))]);
    assert_eq!(block_on(d).unwrap().heads, vec![(thread(4), seq(1))]);

    assert_eq!(read(&store, thread(1))[2..], c_commit.events);
    assert_eq!(read(&store, thread(2)), Vec::<Envelope>::new());
    assert_eq!(read(&store, thread(3)), Vec::<Envelope>::new());
    let missing = block_on(store.read_blob(blob.hash(), 0, 8));
    assert!(
        matches!(missing, Err(Error::BlobMissing { .. })),
        "{missing:?}"
    );
    assert!(block_on(store.receipt(id(2))).unwrap().is_none());
    let mut heads = vec![(thread(1), seq(4)), (thread(4), seq(1))];
    heads.sort_unstable();
    assert_eq!(block_on(store.heads()).unwrap(), heads);
    block_on(store.close()).unwrap();

    let conn = sql(&dir);
    assert_eq!(count(&conn, "SELECT count(*) FROM streams"), 2);
    assert_eq!(count(&conn, "SELECT count(*) FROM blobs"), 0);
    assert_eq!(count(&conn, "SELECT count(*) FROM events"), 5);
}

#[test]
fn pipelined_commits_of_one_stream_share_a_group() {
    let dir = TestDir::new("pipelined_commits_of_one_stream_share_a_group");
    let store = dir.open();
    let replies: Vec<CommitReply> = grouped(&store, || {
        (1..=8)
            .map(|n| send(&store, texts(thread(1), n, n)))
            .collect()
    });
    for (n, reply) in (1..).zip(replies) {
        assert_eq!(block_on(reply).unwrap().heads, vec![(thread(1), seq(n))]);
    }
    assert_eq!(seqs(&read(&store, thread(1))), (1..=8).collect::<Vec<_>>());
    block_on(store.close()).unwrap();
}

#[test]
fn commit_after_failed_pipelined_commit_gets_seq_gap() {
    let dir = TestDir::new("commit_after_failed_pipelined_commit_gets_seq_gap");
    let store = dir.open();
    let mut first = texts(thread(1), 1, 1);
    first.receipt = Some(receipt(1, Some(thread(1))));
    block_on(store.commit(first)).unwrap();

    let mut a = texts(thread(1), 2, 2);
    a.receipt = Some(receipt(1, Some(thread(1))));
    let (a, b) = grouped(&store, || {
        (send(&store, a), send(&store, texts(thread(1), 3, 3)))
    });
    let a = block_on(a);
    let first = receipt(1, Some(thread(1)));
    assert!(
        matches!(&a, Err(Error::DuplicateCommand { stored }) if **stored == first),
        "{a:?}"
    );
    assert_seq_gap(block_on(b), thread(1), 2, 3);

    assert_eq!(seqs(&read(&store, thread(1))), vec![1]);
    assert_eq!(block_on(store.heads()).unwrap(), vec![(thread(1), seq(1))]);
    // The stream goes on from the last seq written.
    block_on(store.commit(texts(thread(1), 2, 2))).unwrap();
    block_on(store.close()).unwrap();
    assert_eq!(count(&sql(&dir), "SELECT count(*) FROM events"), 2);
}

/// What a commit wrote: its result and the events it carried for one stream.
type Outcome = (StreamId, Vec<Envelope>, Result<Committed, Error>);

/// Every Ok commit reads back whole and is a head; every failed one left its stream empty.
fn assert_store_matches(store: &Store, outcomes: &[Outcome]) {
    let mut heads = Vec::new();
    for (stream, events, result) in outcomes {
        if result.is_ok() {
            assert_eq!(&read(store, *stream), events, "{stream}");
            heads.push((*stream, events.last().unwrap().seq));
        } else {
            assert_eq!(read(store, *stream), Vec::<Envelope>::new(), "{stream}");
        }
    }
    heads.sort_unstable();
    assert_eq!(block_on(store.heads()).unwrap(), heads);
}

#[test]
fn disk_full_inside_a_group_keeps_results_and_file_consistent() {
    let dir = TestDir::new("disk_full_inside_a_group_keeps_results_and_file_consistent");
    let options = StoreOptions {
        max_page_count: Some(40),
        ..StoreOptions::new(dir.path())
    };
    let store = block_on(Store::open(options)).unwrap();
    let big = BlobIn::new(vec![7; 2 * 1024 * 1024]);
    let big_hash = big.hash();
    let a = texts(thread(1), 1, 2);
    let b = Commit {
        blobs: vec![big],
        ..texts(thread(2), 1, 1)
    };
    let c = texts(thread(3), 1, 2);
    let commits = [a, b, c];
    let replies: Vec<CommitReply> = grouped(&store, || {
        commits
            .iter()
            .map(|commit| send(&store, commit.clone()))
            .collect()
    });
    let outcomes: Vec<Outcome> = commits
        .iter()
        .zip(replies)
        .map(|(commit, reply)| {
            (
                commit.events[0].stream,
                commit.events.clone(),
                block_on(reply),
            )
        })
        .collect();

    // SQLite may roll the whole transaction back on SQLITE_FULL; then A fails with B's error
    // and C runs in a new transaction.
    assert!(
        matches!(&outcomes[0].2, Ok(_) | Err(Error::DiskFull)),
        "{:?}",
        outcomes[0].2
    );
    assert!(
        matches!(&outcomes[1].2, Err(Error::DiskFull)),
        "{:?}",
        outcomes[1].2
    );
    assert_eq!(
        outcomes[2].2.as_ref().unwrap().heads,
        vec![(thread(3), seq(2))]
    );
    assert_store_matches(&store, &outcomes);
    let missing = block_on(store.read_blob(big_hash, 0, 8));
    assert!(
        matches!(missing, Err(Error::BlobMissing { .. })),
        "{missing:?}"
    );
    block_on(store.close()).unwrap();

    let store = dir.open();
    assert_store_matches(&store, &outcomes);
    block_on(store.close()).unwrap();
}

#[test]
fn commits_around_a_queued_non_commit_request_both_land() {
    let dir = TestDir::new("commits_around_a_queued_non_commit_request_both_land");
    let store = dir.open();
    let limits = AccountLimits {
        agent: AgentKind::Claude,
        account: AccountRef("acct-a".to_owned()),
        plan: None,
        billing: Billing::Plan,
        windows: Vec::new(),
        at_limit: AtLimitBehaviour::Pauses,
        source: LimitSource::Loading,
    };
    let release = block_on(store.hold_writer()).unwrap();
    let a = send(&store, texts(thread(1), 1, 1));
    // join! polls the put first, which queues it after A, then queues C (seq 2 of A's stream)
    // and releases the writer: the queue holds A, the put, C, in that order.
    let (put, c) = block_on(async {
        futures::join!(store.put_account_limits(limits.clone()), async {
            let c = store.send_commit(texts(thread(1), 2, 2)).await.unwrap();
            release.send(()).unwrap();
            c.await
        })
    });
    put.unwrap();
    assert_eq!(block_on(a).unwrap().heads, vec![(thread(1), seq(1))]);
    assert_eq!(c.unwrap().heads, vec![(thread(1), seq(2))]);
    assert_eq!(seqs(&read(&store, thread(1))), vec![1, 2]);
    assert_eq!(block_on(store.account_limits()).unwrap(), vec![limits]);
    block_on(store.close()).unwrap();
}
