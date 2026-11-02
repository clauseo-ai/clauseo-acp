//! OS lookups that miss the cache: one per key at a time, at most four at once, and the bounded
//! thread a Windows lookup runs on.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::super::Scheme::Https;
use super::super::{Env, Lookup, OS_LOOKUPS_MOST, Route, Upstream, bounded};
use super::{FakeClock, http};

/// A lookup whose OS answer waits for the test: each call reports its host on `arrived` and
/// returns once the test sends on the matching release channel.
struct Gated {
    lookup: Arc<Lookup>,
    calls: Arc<AtomicUsize>,
    arrived: Receiver<String>,
    release: Sender<()>,
}

fn gated() -> Gated {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let (arrived_tx, arrived) = mpsc::channel();
    let (release, release_rx) = mpsc::channel::<()>();
    let arrived_tx = Mutex::new(arrived_tx);
    let release_rx = Mutex::new(release_rx);
    let lookup = Lookup::with_parts(
        Upstream::System,
        Env::default(),
        move |_: super::super::Scheme, host: &str, _: u16| {
            counted.fetch_add(1, Ordering::SeqCst);
            arrived_tx.lock().unwrap().send(host.to_owned()).unwrap();
            if release_rx.lock().unwrap().recv().is_err() {
                // The test ended first.
            }
            http("os-proxy", 1)
        },
        FakeClock::default(),
    )
    .unwrap();
    Gated {
        lookup: Arc::new(lookup),
        calls,
        arrived,
        release,
    }
}

fn route_on_thread(lookup: &Arc<Lookup>, host: &str) -> thread::JoinHandle<Route> {
    let lookup = Arc::clone(lookup);
    let host = host.to_owned();
    thread::spawn(move || lookup.route(Https, &host, 443))
}

#[test]
fn a_second_miss_for_the_same_host_waits_for_the_first_lookup() {
    let gated = gated();
    let first = route_on_thread(&gated.lookup, "a.test");
    assert_eq!(gated.arrived.recv().unwrap(), "a.test");
    let second = route_on_thread(&gated.lookup, "a.test");
    // Spin (no sleep) until the second caller is parked on the first's lookup.
    while gated.lookup.waiting() == 0 {
        thread::yield_now();
    }
    gated.release.send(()).unwrap();
    assert_eq!(first.join().unwrap(), http("os-proxy", 1));
    assert_eq!(second.join().unwrap(), http("os-proxy", 1));
    assert_eq!(
        gated.calls.load(Ordering::SeqCst),
        1,
        "one OS lookup for both"
    );
}

#[test]
fn past_four_lookups_at_once_a_miss_goes_direct_uncached() {
    let gated = gated();
    let running: Vec<_> = (0..OS_LOOKUPS_MOST)
        .map(|i| {
            let handle = route_on_thread(&gated.lookup, &format!("h{i}.test"));
            gated.arrived.recv().unwrap();
            handle
        })
        .collect();
    assert_eq!(OS_LOOKUPS_MOST, 4);
    assert_eq!(gated.lookup.route(Https, "fifth.test", 443), Route::Direct);
    assert_eq!(gated.calls.load(Ordering::SeqCst), 4, "the fifth never ran");
    for _ in 0..OS_LOOKUPS_MOST {
        gated.release.send(()).unwrap();
    }
    for handle in running {
        assert_eq!(handle.join().unwrap(), http("os-proxy", 1));
    }
    // The direct answer was not cached: the next miss asks the OS.
    gated.release.send(()).unwrap();
    assert_eq!(
        gated.lookup.route(Https, "fifth.test", 443),
        http("os-proxy", 1)
    );
    assert_eq!(gated.calls.load(Ordering::SeqCst), 5);
}

#[test]
fn a_bounded_lookup_times_out_and_lingering_threads_are_capped() {
    static THREADS: AtomicUsize = AtomicUsize::new(0);
    let (release, wait) = mpsc::channel::<()>();
    let hung = bounded(Duration::from_millis(50), &THREADS, 1, move || {
        if wait.recv().is_err() {
            // Released by the test dropping its end.
        }
        Ok(Route::Direct)
    });
    assert!(hung.unwrap_err().contains("took over"));
    // The timed-out lookup still holds its thread: no second one starts.
    let refused = bounded(Duration::from_secs(10), &THREADS, 1, || Ok(Route::Direct));
    assert!(refused.unwrap_err().contains("still running"));
    drop(release);
    let fast = bounded(Duration::from_secs(10), &THREADS, 2, || Ok(http("p", 1)));
    assert_eq!(fast.unwrap(), http("p", 1));
}

#[test]
fn a_lookup_that_panics_gives_its_thread_slot_back() {
    static THREADS: AtomicUsize = AtomicUsize::new(0);
    // The thread unwinds: its slot guard drops before the answer's sender, so by the time the
    // waiter sees the sender gone, the count is back to zero.
    let failed = bounded(Duration::from_secs(10), &THREADS, 1, || {
        std::panic::resume_unwind(Box::new("the OS lookup failed"))
    });
    assert!(failed.is_err());
    assert_eq!(THREADS.load(std::sync::atomic::Ordering::SeqCst), 0);
    let next = bounded(Duration::from_secs(10), &THREADS, 1, || Ok(http("p", 1)));
    assert_eq!(next.unwrap(), http("p", 1));
}
