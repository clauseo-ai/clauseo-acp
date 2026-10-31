//! Tests of the commit pipeline against a real store in a temp folder, driven on a `LocalPool`.

pub(crate) mod support;
mod write;

mod inflight;

use clauseo_protocol::{CommandId, CommandOutcome, Seq, StreamId};
use clauseo_reducer::StreamState;
use clauseo_store::{Receipt, Store};
use futures::executor::LocalPool;

use self::support::{Results, TempDir, id, ids, notice, open_store, thread_stream, ts};
use super::{Completed, Draft, FeederTx, Pipeline, Ticket, spawn_feeder};
use crate::ids::Ids;

/// A receipt for command `n`, with no stream.
fn receipt(n: u64) -> Receipt {
    Receipt {
        command_id: id::<CommandId>(n),
        stream: None,
        payload_hash: [7; 32],
        result: CommandOutcome::Done,
        ts: ts(0),
    }
}

/// Sends one commit of one notice in `stream`, with `receipt` when given, tagged `tag`.
fn send_notice(
    pipeline: &mut Pipeline<usize>,
    feeder: &FeederTx,
    ids: &mut Ids,
    stream: StreamId,
    tag: usize,
    receipt: Option<Receipt>,
) -> (Ticket, Seq) {
    let mut draft = Draft::new();
    let seq = pipeline
        .push(&mut draft, stream, notice(tag), None, ts(0), ids)
        .unwrap();
    if let Some(receipt) = receipt {
        draft.receipt(receipt);
    }
    (pipeline.send(draft, tag, feeder).unwrap(), seq)
}

/// The seqs of the commits `completed` released `Ok`, and the tags of all it released.
fn released(completed: &Completed<usize>) -> Vec<usize> {
    completed.done.iter().map(|done| done.tag).collect()
}

/// The stored seqs of `stream`.
fn stored_seqs(pool: &mut LocalPool, store: &Store, stream: StreamId) -> Vec<u64> {
    pool.run_until(store.read_after(stream, None, 500))
        .unwrap()
        .iter()
        .map(|envelope| envelope.seq.get())
        .collect()
}

#[test]
fn pipelined_commits_keep_seq_order() {
    let dir = TempDir::new("pipelined_commits_keep_seq_order");
    let mut pool = LocalPool::new();
    let store = open_store(&mut pool, &dir);
    let (post, mut results) = Results::new();
    let feeder = spawn_feeder(&pool.spawner(), store.clone(), post).unwrap();
    let mut pipeline = Pipeline::<usize>::new(pool.run_until(store.heads()).unwrap());
    let mut ids = ids();
    let stream = thread_stream();

    // Hold the writer so all 8 commits are queued before the first result can return.
    let release = pool.run_until(store.hold_writer()).unwrap();
    let mut tickets = Vec::new();
    for tag in 0..8 {
        let (ticket, seq) = send_notice(&mut pipeline, &feeder, &mut ids, stream, tag, None);
        assert_eq!(seq.get(), u64::try_from(tag).unwrap() + 1);
        tickets.push(ticket);
    }
    pool.run_until_stalled();
    assert_eq!(
        store.queued_writes(),
        8,
        "all 8 commits are in the writer's queue"
    );
    assert!(
        results.ready().is_none(),
        "no result before the writer runs"
    );
    assert_eq!(pipeline.pending_head(stream), Seq::new(8).ok());
    assert_eq!(pipeline.durable_head(stream), None);
    release.send(()).unwrap();

    let mut posted = Vec::new();
    for _ in 0..8 {
        let (ticket, result) = results.next(&mut pool);
        posted.push((ticket, result.unwrap()));
    }
    // Feed the results in a scrambled order: each commit is released only after its own result
    // and every earlier one, so the release order is the seq order.
    let order = [2, 0, 1, 3, 4, 7, 6, 5];
    let expected: [&[usize]; 8] = [&[], &[0], &[1, 2], &[3], &[4], &[], &[], &[5, 6, 7]];
    let mut state = StreamState::new(stream);
    let mut published = Vec::new();
    for (index, want) in order.iter().zip(expected) {
        let (ticket, committed) = posted.iter().find(|(t, _)| *t == tickets[*index]).unwrap();
        let completed = pipeline.on_result(*ticket, Ok(committed.clone()));
        assert_eq!(released(&completed), want);
        assert_eq!(completed.unblocked, []);
        for done in completed.done {
            for envelope in done.outcome.unwrap() {
                clauseo_reducer::apply(&mut state, &envelope).unwrap();
                published.push(envelope.seq.get());
            }
        }
    }
    assert_eq!(published, (1..=8).collect::<Vec<_>>());
    assert_eq!(state.seq(), Seq::new(8).ok());
    assert_eq!(pipeline.durable_head(stream), Seq::new(8).ok());
    assert_eq!(stored_seqs(&mut pool, &store, stream), published);

    drop(feeder);
    pool.run_until(store.close()).unwrap();
}

#[test]
fn failed_commit_fails_later_pipelined_commits() {
    let dir = TempDir::new("failed_commit_fails_later_pipelined_commits");
    let mut pool = LocalPool::new();
    let store = open_store(&mut pool, &dir);
    let (post, mut results) = Results::new();
    let feeder = spawn_feeder(&pool.spawner(), store.clone(), post).unwrap();
    let mut pipeline = Pipeline::<usize>::new(pool.run_until(store.heads()).unwrap());
    let mut ids = ids();
    let stream = thread_stream();

    // A receipt for command 1 is stored first, with no events.
    let mut draft = Draft::new();
    draft.receipt(receipt(1));
    let first = pipeline.send(draft, 100, &feeder).unwrap();
    let (ticket, result) = results.next(&mut pool);
    assert_eq!(ticket, first);
    let completed = pipeline.on_result(ticket, result);
    assert!(completed.done[0].outcome.is_ok());

    // Commit 3 of 8 reuses command 1's id.
    let release = pool.run_until(store.hold_writer()).unwrap();
    let mut tickets = Vec::new();
    for tag in 0..8 {
        let reused = (tag == 2).then(|| receipt(1));
        tickets.push(send_notice(&mut pipeline, &feeder, &mut ids, stream, tag, reused).0);
    }
    release.send(()).unwrap();

    let mut published = Vec::new();
    let mut failures = Vec::new();
    let mut unblocked = Vec::new();
    for arrived in 0..8 {
        let (ticket, result) = results.next(&mut pool);
        let tag = tickets.iter().position(|t| *t == ticket).unwrap();
        match (tag, &result) {
            (0 | 1, Ok(_))
            | (2, Err(clauseo_store::Error::DuplicateCommand { .. }))
            | (3.., Err(clauseo_store::Error::SeqGap { .. })) => {}
            other => panic!("unexpected result {other:?}"),
        }
        let completed = pipeline.on_result(ticket, result);
        if arrived < 7 {
            assert_eq!(completed.unblocked, []);
        }
        if !failures.is_empty() && arrived < 7 {
            assert!(
                pipeline.blocked(stream).is_some(),
                "blocked until all 8 returned"
            );
            let mut draft = Draft::new();
            assert!(
                pipeline
                    .push(&mut draft, stream, notice(9), None, ts(0), &mut ids)
                    .is_err()
            );
        }
        unblocked.extend(completed.unblocked);
        for done in completed.done {
            match done.outcome {
                Ok(envelopes) => published.extend(envelopes.iter().map(|e| e.seq.get())),
                Err(failure) => failures.push((done.tag, failure)),
            }
        }
    }
    assert_eq!(published, [1, 2]);
    assert_eq!(failures.len(), 6);
    let reported: Vec<usize> = failures
        .iter()
        .filter(|(_, failure)| failure.first)
        .map(|(tag, _)| *tag)
        .collect();
    assert_eq!(reported, [2], "one failure is reported, commit 3's");
    for (_, failure) in &failures {
        assert!(
            matches!(failure.error, clauseo_store::Error::DuplicateCommand { .. }),
            "every failed commit answers with commit 3's error"
        );
    }
    assert_eq!(unblocked, [stream]);
    assert!(pipeline.blocked(stream).is_none());
    assert_eq!(pipeline.pending_head(stream), Seq::new(2).ok());

    // The next commit, sent after all 8 returned, takes seq 3.
    let (ticket, seq) = send_notice(&mut pipeline, &feeder, &mut ids, stream, 8, None);
    assert_eq!(seq.get(), 3);
    let (posted, result) = results.next(&mut pool);
    assert_eq!(posted, ticket);
    let completed = pipeline.on_result(posted, result);
    assert!(completed.done[0].outcome.is_ok());
    assert_eq!(stored_seqs(&mut pool, &store, stream), [1, 2, 3]);

    drop(feeder);
    pool.run_until(store.close()).unwrap();
}
