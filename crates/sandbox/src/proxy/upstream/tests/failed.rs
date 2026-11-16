//! A cached failure: it is one host's alone, and a success after it expires replaces it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::super::Scheme::Https;
use super::super::{Env, FailKind, Lookup, Upstream, UpstreamAnswer};
use super::{FakeClock, http};

/// A lookup whose OS answers `answer(host)`, counting calls per host.
fn per_host(
    answer: impl Fn(&str) -> UpstreamAnswer + Send + Sync + 'static,
) -> (Lookup, Arc<Mutex<Vec<String>>>, FakeClock) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&asked);
    let clock = FakeClock::default();
    let lookup = Lookup::with_parts(
        Upstream::System,
        Env::default(),
        move |_: super::super::Scheme, host: &str, _: u16| {
            seen.lock().unwrap().push(host.to_owned());
            answer(host)
        },
        clock.clone(),
    )
    .unwrap();
    (lookup, asked, clock)
}

fn asked_for(asked: &Mutex<Vec<String>>, host: &str) -> usize {
    asked.lock().unwrap().iter().filter(|h| *h == host).count()
}

/// One host's failed lookup leaves another host's cached answer as it was.
#[test]
fn a_hosts_failure_leaves_another_hosts_cached_answer_alone() {
    let (lookup, asked, _clock) = per_host(|host| {
        if host == "bad.test" {
            UpstreamAnswer::Failed(FailKind::Other)
        } else {
            http("os-proxy", 1).into()
        }
    });
    let good: UpstreamAnswer = http("os-proxy", 1).into();
    assert_eq!(lookup.answer(Https, "good.test", 443), good);
    assert_eq!(
        lookup.answer(Https, "bad.test", 443),
        UpstreamAnswer::Failed(FailKind::Other)
    );
    assert_eq!(lookup.answer(Https, "good.test", 443), good);
    assert_eq!(asked_for(&asked, "good.test"), 1, "still the cached answer");
    assert_eq!(
        lookup.answer(Https, "bad.test", 443),
        UpstreamAnswer::Failed(FailKind::Other)
    );
    assert_eq!(
        asked_for(&asked, "bad.test"),
        1,
        "the failure is cached too"
    );
}

/// A cached failure holds for 60 s; the success after it expires replaces it, and is cached.
#[test]
fn a_success_after_expiry_replaces_a_cached_failure() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let (lookup, _asked, clock) = per_host(move |_host| {
        if counted.fetch_add(1, Ordering::SeqCst) == 0 {
            UpstreamAnswer::Failed(FailKind::Other)
        } else {
            http("os-proxy", 1).into()
        }
    });
    assert_eq!(
        lookup.answer(Https, "example.test", 443),
        UpstreamAnswer::Failed(FailKind::Other)
    );
    clock.advance(Duration::from_secs(59));
    assert_eq!(
        lookup.answer(Https, "example.test", 443),
        UpstreamAnswer::Failed(FailKind::Other)
    );
    clock.advance(Duration::from_secs(1));
    let proxied: UpstreamAnswer = http("os-proxy", 1).into();
    assert_eq!(lookup.answer(Https, "example.test", 443), proxied);
    assert_eq!(lookup.answer(Https, "example.test", 443), proxied);
    assert_eq!(calls.load(Ordering::SeqCst), 2, "the success is cached");
}

/// The egress proxy's probe is [`reachable`](super::super::reachable) unless a test swaps it: a
/// bound loopback listener answers within the cap, a closed port does not.
#[test]
fn the_lookups_probe_is_the_real_one() {
    let cap = Duration::from_secs(1);
    let lookup = Lookup::new(Upstream::System).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let open = listener.local_addr().unwrap().port();
    assert!(lookup.reachable("127.0.0.1", open, cap), "a listener");
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    assert!(!lookup.reachable("127.0.0.1", closed, cap), "a closed port");
}
