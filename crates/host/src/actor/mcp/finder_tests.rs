//! The finder's bounds: a slow one times out, and no second worker starts while it runs.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use super::bounded;
use crate::config::mcp::McpFinder;

/// Waits, in 5 ms steps up to 5 s, for `busy` to clear; panics with `what` when it does not.
fn until_free(busy: &AtomicBool, what: &str) {
    let (_keep, tick) = mpsc::channel::<()>();
    for _ in 0..1000 {
        if !busy.load(Ordering::SeqCst) {
            return;
        }
        let _gone = tick.recv_timeout(Duration::from_millis(5));
    }
    panic!("{what}");
}

#[test]
fn a_slow_finder_times_out_and_none_other_starts_until_it_ends() {
    let (release, wait) = mpsc::channel::<()>();
    let wait = std::sync::Mutex::new(wait);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let find: McpFinder = Arc::new(move |_: &[String]| {
        counted.fetch_add(1, Ordering::SeqCst);
        let _gone = wait.lock().unwrap().recv();
        Vec::new()
    });
    let busy = Arc::new(AtomicBool::new(false));
    let short = Duration::from_millis(50);
    assert!(
        bounded(&find, Vec::new(), &busy, short).is_none(),
        "it timed out"
    );
    assert!(
        busy.load(Ordering::SeqCst),
        "the stuck worker still holds the flag"
    );
    assert!(bounded(&find, Vec::new(), &busy, short).is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1, "no second worker started");
    release.send(()).unwrap();
    until_free(&busy, "the worker never ended");
    release.send(()).unwrap();
    let found = bounded(&find, Vec::new(), &busy, Duration::from_secs(5));
    assert_eq!(found.map(|found| found.len()), Some(0));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn a_panicking_finder_answers_none_and_frees_the_flag() {
    let find: McpFinder = Arc::new(|_: &[String]| panic!("a finder bug"));
    let busy = Arc::new(AtomicBool::new(false));
    assert!(bounded(&find, Vec::new(), &busy, Duration::from_secs(5)).is_none());
    until_free(&busy, "the flag stayed set");
}
