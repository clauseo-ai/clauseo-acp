//! The accept loop under a lasting accept failure: paused with a doubling backoff on the host's
//! clock, logged once per streak.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::executor::block_on;

use super::super::Phase;
use super::super::accept::{BACKOFF_MOST, accept_loop};
use super::support::*;

#[test]
fn a_lasting_accept_failure_backs_off_and_is_logged_once() {
    let rig = rig();
    let session = Arc::clone(rig.session.shared());
    let captured = Captured::default();
    let dispatch = captured.dispatch();
    let calls = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&calls);
    let looping = Arc::clone(&session);
    let thread = std::thread::spawn(move || {
        tracing::dispatcher::with_default(&dispatch, || {
            accept_loop(&looping, || {
                counted.fetch_add(1, Ordering::SeqCst);
                Err(io::Error::from_raw_os_error(24))
            });
        });
    });
    let expected = [10, 20, 40, 80, 160, 320, 640, 1000, 1000, 1000].map(Duration::from_millis);
    for (n, pause) in expected.iter().enumerate() {
        // Each failure arms one pause and waits for it: no retry before the clock moves.
        assert_eq!(block_on(rig.clock.next_armed()), *pause, "pause {n}");
        assert_eq!(calls.load(Ordering::SeqCst), n as u64 + 1);
        if n + 1 == expected.len() {
            // Stopped while paused: the next try sees it and the loop returns.
            session.close(Phase::Stopped);
        }
        rig.clock.advance(*pause);
    }
    assert_eq!(*expected.last().unwrap(), BACKOFF_MOST);
    thread.join().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), expected.len() as u64 + 1);
    let log = captured.text();
    assert_eq!(log.matches("accept failed").count(), 1, "{log}");
}
