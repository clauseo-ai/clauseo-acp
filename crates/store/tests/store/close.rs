//! Closing and dropping: a call still queued when the store closes gets `Closed`, and dropping
//! the last handle without `close` serves what is queued, then releases the file.
//!
//! The order is made certain, not timed: the writer is held with `Store::hold_writer`, and
//! `close` is polled by hand until `Store::queued_writes` shows its `Close` in the writer's
//! queue, before the commit behind it is sent.

use std::future::Future;
use std::sync::{Arc, Condvar, Mutex};
use std::task::Context;

use clauseo_store::Error;
use futures::executor::block_on;
use futures::task::ArcWake;

use crate::common::{TestDir, seq, seqs, texts, thread};

/// A waker that records a wake, and lets the test wait for one.
#[derive(Default)]
struct Woken {
    woken: Mutex<bool>,
    signal: Condvar,
}

impl ArcWake for Woken {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        *arc_self.woken.lock().unwrap() = true;
        arc_self.signal.notify_all();
    }
}

impl Woken {
    /// Blocks until the next wake, then clears it.
    fn wait(&self) {
        let mut woken = self.woken.lock().unwrap();
        while !*woken {
            woken = self.signal.wait(woken).unwrap();
        }
        *woken = false;
    }
}

#[test]
fn commit_queued_behind_close_gets_closed() {
    let dir = TestDir::new("commit_queued_behind_close_gets_closed");
    let store = dir.open();
    block_on(store.commit(texts(thread(1), 1, 1))).unwrap();
    let release = block_on(store.hold_writer()).unwrap();

    let woken = Arc::new(Woken::default());
    let waker = futures::task::waker(Arc::clone(&woken));
    let mut cx = Context::from_waker(&waker);
    let mut closing = Box::pin(store.clone().close());
    // The first poll sends the reader's `Close`. If the reader answers within that poll, it
    // also queues the writer's; else the reader's answer wakes us, and the next poll does.
    assert!(closing.as_mut().poll(&mut cx).is_pending());
    if store.queued_writes() == 0 {
        woken.wait();
        assert!(closing.as_mut().poll(&mut cx).is_pending());
    }
    assert_eq!(
        store.queued_writes(),
        1,
        "the writer's Close waits behind the hold"
    );

    let behind = block_on(store.send_commit(texts(thread(1), 2, 2))).unwrap();
    assert_eq!(store.queued_writes(), 2);
    release.send(()).unwrap();
    assert!(matches!(block_on(behind), Err(Error::Closed)));
    assert!(matches!(
        block_on(store.send_commit(texts(thread(1), 2, 2))),
        Err(Error::Closed)
    ));
    block_on(closing).unwrap();
    drop(store);

    // Nothing of the commit behind `Close` was written.
    let store = dir.open();
    assert_eq!(block_on(store.heads()).unwrap(), vec![(thread(1), seq(1))]);
    block_on(store.close()).unwrap();
}

#[test]
fn drop_without_close_serves_queue_and_releases_file() {
    let dir = TestDir::new("drop_without_close_serves_queue_and_releases_file");
    let store = dir.open();
    block_on(store.commit(texts(thread(1), 1, 1))).unwrap();
    // Queued, then every handle dropped before its reply is awaited.
    let queued = block_on(store.send_commit(texts(thread(1), 2, 3))).unwrap();
    drop(store);
    let committed = block_on(queued).unwrap();
    assert_eq!(committed.heads, vec![(thread(1), seq(3))]);

    // The lock is gone by the time drop returns.
    let store = dir.open();
    let reopen = block_on(store.reopen(thread(1), 1)).unwrap();
    assert_eq!(seqs(&reopen.tail), vec![1, 2, 3]);
    block_on(store.close()).unwrap();
}
