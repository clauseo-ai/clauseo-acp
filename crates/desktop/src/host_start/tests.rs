//! The host's start off the UI thread, held by the test: what a call sent meanwhile gets, after
//! a start that worked and one that failed, and what exit does in each case.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use clauseo_host::{Host, HostConfig};
use clauseo_protocol::{ErrorCode, HostQuery, QueryResult};

use super::*;
use crate::bridge::Bridge;
use crate::test_support::{Scratch, collector};

/// How long a test waits for a thread it started. A bound, not a sleep.
const THREAD_WAIT: Duration = Duration::from_secs(20);

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    futures::executor::block_on(future)
}

/// A bridge whose host starts with `config` on its own thread once the returned sender sends
/// (or hangs up): the test's hold on the start.
fn held_start(config: HostConfig) -> (Arc<HostCell>, Bridge, Sender<()>) {
    let (cell, start) = HostCell::new();
    let (release, held) = mpsc::channel::<()>();
    start_on_thread(start, move || {
        let _released = held.recv();
        Host::start(config)
    })
    .unwrap();
    let bridge = Bridge::new(Arc::clone(&cell));
    (cell, bridge, release)
}

/// Runs `stop(cell)` on a thread of its own; the receiver hears when it returned.
fn stop_on_thread(cell: &Arc<HostCell>) -> Receiver<()> {
    let (stopped, done) = mpsc::channel();
    let cell = Arc::clone(cell);
    std::thread::spawn(move || {
        stop(&cell);
        let _heard = stopped.send(());
    });
    done
}

#[test]
fn a_query_sent_while_the_host_starts_waits_and_is_answered() {
    let scratch = Scratch::new("held-start", &[]);
    let (cell, bridge, release) = held_start(scratch.config(Vec::new()));

    let mut query = Box::pin(bridge.query(HostQuery::ListMatters));
    assert!(
        (&mut query).now_or_never().is_none(),
        "answered before the host started"
    );
    release.send(()).unwrap();
    let answer = block_on(query).unwrap();
    assert!(
        matches!(&answer, QueryResult::Matters(matters) if !matters.is_empty()),
        "{answer:?}"
    );

    stop(&cell);
}

#[test]
fn after_a_failed_start_every_call_gets_its_store_code() {
    let scratch = Scratch::new("failed-start", &[]);
    // A host already holds the data folder, so the held start fails with `store.locked`.
    let first = scratch.start(Vec::new());
    let (cell, bridge, release) = held_start(scratch.config(Vec::new()));

    let mut waiting = Box::pin(bridge.query(HostQuery::ListMatters));
    assert!((&mut waiting).now_or_never().is_none());
    release.send(()).unwrap();
    let refusal = block_on(waiting).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::StoreLocked, "{refusal:?}");

    let later = block_on(bridge.query(HostQuery::GetSettings)).unwrap_err();
    assert_eq!(later.code(), ErrorCode::StoreLocked, "{later:?}");
    let (channel, _messages) = collector();
    let stream = block_on(bridge.open_stream("main", channel)).unwrap_err();
    assert_eq!(stream.code(), ErrorCode::StoreLocked, "{stream:?}");

    // Exit after the failed start: nothing to stop, and it returns.
    stop_on_thread(&cell).recv_timeout(THREAD_WAIT).unwrap();
    assert!(lock(&cell.join).is_none());
    // The other host was not touched.
    block_on(first.host.query(
        clauseo_protocol::Principal::LocalDesktop,
        HostQuery::ListMatters,
    ))
    .unwrap();
}

#[test]
fn exit_while_the_host_starts_waits_for_it_then_stops_it() {
    let scratch = Scratch::new("exit-during-start", &[]);
    let (cell, bridge, release) = held_start(scratch.config(Vec::new()));

    let done = stop_on_thread(&cell);
    // Held: the stop cannot have got past the wait for the start.
    assert_eq!(done.try_recv(), Err(TryRecvError::Empty));
    release.send(()).unwrap();
    done.recv_timeout(THREAD_WAIT).unwrap();

    // The start finished and worked: read past the cell's own wait, so a `host()` that skips
    // it cannot hide a stop that returned before the start.
    block_on(cell.done.clone()).unwrap();
    assert!(matches!(cell.started.get(), Some(Ok(_))));
    // The host thread was joined, and the host answers no more: shut down, not unstarted.
    assert!(lock(&cell.join).is_none());
    let after = block_on(bridge.query(HostQuery::ListMatters)).unwrap_err();
    assert_eq!(after.code(), ErrorCode::Internal, "{after:?}");
    assert_ne!(after.message(), NO_OUTCOME, "{after:?}");
}

#[test]
fn a_start_dropped_without_an_outcome_leaves_the_cell_internal() {
    let (cell, start) = HostCell::new();
    // The starting thread unwound (dev and test builds) or setup failed after
    // `HostCell::new`: its `HostStart` is dropped unfinished.
    drop(start);
    let error = block_on(cell.host()).unwrap_err();
    assert_eq!(error.code(), ErrorCode::Internal, "{error:?}");
    assert_eq!(error.message(), NO_OUTCOME);
    // Exit has nothing to stop.
    stop(&cell);
}
