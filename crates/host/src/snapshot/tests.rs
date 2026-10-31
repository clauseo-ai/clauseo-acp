//! `snapshot_compressed_off_actor`: the snapshot thread compresses, the store reads it back.

use std::sync::mpsc;
use std::thread;

use clauseo_protocol::{AgentId, DomainEvent, Envelope, EventId, MatterId, Seq, ThreadCreated};
use clauseo_reducer::{REDUCER_SCHEMA_VERSION, Snapshot, StreamState};
use clauseo_store::Commit;
use futures::executor::LocalPool;

use super::{SnapshotWorker, THREAD_NAME};
use crate::pipeline::tests::support::{TempDir, id, notice, open_store, thread_stream, ts};

/// The test thread's first `n` events: the thread, then notices.
fn events(n: u64) -> Vec<Envelope> {
    let stream = thread_stream();
    (1..=n)
        .map(|seq| {
            let event = if seq == 1 {
                DomainEvent::ThreadCreated(ThreadCreated {
                    matter: id::<MatterId>(2),
                    agent: AgentId::new("fake-agent").unwrap(),
                    title: None,
                })
            } else {
                notice(usize::try_from(seq).unwrap())
            };
            let event_id = id::<EventId>(100 + seq);
            Envelope::new(event_id, stream, Seq::new(seq).unwrap(), ts(0), None, event)
        })
        .collect()
}

#[test]
fn snapshot_compressed_off_actor() {
    let dir = TempDir::new("snapshot_compressed_off_actor");
    let mut pool = LocalPool::new();
    let store = open_store(&mut pool, &dir);
    let stream = thread_stream();
    let log = events(5);
    let mut state = StreamState::new(stream);
    for envelope in &log {
        clauseo_reducer::apply(&mut state, envelope).unwrap();
    }
    pool.run_until(store.commit(Commit {
        events: log,
        ..Commit::default()
    }))
    .unwrap();

    // Encoded on this thread, which stands for the actor; compressed on the snapshot thread.
    let bytes = Snapshot::of(&state).to_bytes().unwrap();
    let (sender, posted) = mpsc::channel();
    let worker = SnapshotWorker::start(Box::new(move |snapshot| {
        let here = thread::current();
        sender
            .send((here.name().map(str::to_owned), here.id(), snapshot))
            .unwrap();
    }))
    .unwrap();
    let seq = Seq::new(5).unwrap();
    worker.submit(stream, seq, bytes.clone()).unwrap();
    worker.stop().unwrap();
    let (name, thread_id, snapshot) = posted.recv().unwrap();
    assert_eq!(name.as_deref(), Some(THREAD_NAME));
    assert_ne!(
        thread_id,
        thread::current().id(),
        "never compressed on the actor's thread"
    );
    assert_eq!((snapshot.stream(), snapshot.seq()), (stream, seq));
    assert!(posted.try_recv().is_err(), "one job, one snapshot");

    // The store takes it as a commit with no events and decompresses it to the same bytes.
    pool.run_until(store.commit(Commit {
        snapshot: Some(snapshot),
        ..Commit::default()
    }))
    .unwrap();
    let reopen = pool
        .run_until(store.reopen(stream, REDUCER_SCHEMA_VERSION))
        .unwrap();
    let stored = reopen.snapshot.unwrap();
    assert_eq!(stored.seq, seq);
    assert_eq!(stored.state, bytes);
    assert_eq!(reopen.tail.len(), 0);

    pool.run_until(store.close()).unwrap();
}

#[test]
fn stop_reports_a_panicked_thread() {
    let worker = SnapshotWorker::start(Box::new(|_| panic!("boom"))).unwrap();
    worker
        .submit(thread_stream(), Seq::FIRST, Vec::new())
        .unwrap();
    // The post panicked, so the thread is gone; stop reports it as internal.
    assert!(matches!(
        worker.stop(),
        Err(crate::error::Error::Internal(_))
    ));
}
