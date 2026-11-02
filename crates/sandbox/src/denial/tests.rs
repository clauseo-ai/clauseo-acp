use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::*;

/// A clock the test moves by hand, in milliseconds.
#[derive(Default)]
struct HandClock(AtomicU64);

impl HandClock {
    fn set(&self, ms: u64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}

impl Clock for &HandClock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst))
    }
}

/// A sink that keeps what it gets.
#[derive(Default)]
struct Kept {
    denials: Mutex<Vec<Denial>>,
    upstream: Mutex<Vec<Reachability>>,
}

impl Kept {
    fn denials(&self) -> Vec<Denial> {
        self.denials.lock().unwrap().clone()
    }
}

impl DenialSink for Kept {
    fn denied(&self, d: Denial) {
        self.denials.lock().unwrap().push(d);
    }

    fn upstream(&self, r: Reachability) {
        self.upstream.lock().unwrap().push(r);
    }
}

fn path_denial(n: usize) -> Denial {
    Denial::Path {
        path: PathBuf::from(format!("/outside/{n}")),
        access: Access::Read,
        class: PathClass::Outside,
    }
}

#[test]
fn twenty_pass_and_the_twenty_first_is_dropped_at_the_next_window() {
    let clock = HandClock::default();
    let cap = CappedSink::with_clock(Kept::default(), &clock);
    for n in 0..21 {
        clock.set(n as u64 * 10);
        cap.denied(path_denial(n));
    }
    let got = cap.inner().denials();
    assert_eq!(got.len(), 20);
    assert_eq!(got.last(), Some(&path_denial(19)));

    clock.set(1_000);
    cap.denied(path_denial(100));
    let got = cap.inner().denials();
    assert_eq!(got.len(), 22);
    assert_eq!(got[20], Denial::Dropped { count: 1 });
    assert_eq!(got[21], path_denial(100));
}

#[test]
fn tick_delivers_the_count_with_no_later_denial() {
    let clock = HandClock::default();
    let cap = CappedSink::with_clock(Kept::default(), &clock);
    for n in 0..25 {
        cap.denied(path_denial(n));
    }
    clock.set(999);
    cap.tick();
    assert_eq!(cap.inner().denials().len(), 20, "the window still runs");

    clock.set(1_000);
    cap.tick();
    let got = cap.inner().denials();
    assert_eq!(got.len(), 21);
    assert_eq!(got[20], Denial::Dropped { count: 5 });

    clock.set(2_500);
    cap.tick();
    assert_eq!(
        cap.inner().denials().len(),
        21,
        "nothing held, nothing sent"
    );
}

#[test]
fn a_new_window_starts_at_the_first_denial_after_a_quiet_spell() {
    let clock = HandClock::default();
    let cap = CappedSink::with_clock(Kept::default(), &clock);
    cap.denied(path_denial(0));
    clock.set(5_000);
    for n in 1..=20 {
        cap.denied(path_denial(n));
    }
    let got = cap.inner().denials();
    assert_eq!(got.len(), 21, "a fresh window passes 20 more");
    assert!(!got.iter().any(|d| matches!(d, Denial::Dropped { .. })));
}

#[test]
fn upstream_passes_through() {
    let clock = HandClock::default();
    let cap = CappedSink::with_clock(Kept::default(), &clock);
    cap.upstream(Reachability::Unreachable);
    assert_eq!(
        *cap.inner().upstream.lock().unwrap(),
        vec![Reachability::Unreachable]
    );
}

#[test]
fn many_threads_share_one_cap() {
    let clock = HandClock::default();
    let cap = CappedSink::with_clock(Kept::default(), &clock);
    std::thread::scope(|scope| {
        for t in 0..8 {
            let cap = &cap;
            scope.spawn(move || {
                for n in 0..10 {
                    cap.denied(path_denial(t * 10 + n));
                }
            });
        }
    });
    clock.set(1_000);
    cap.tick();
    let got = cap.inner().denials();
    assert_eq!(got.len(), 21);
    assert_eq!(got[20], Denial::Dropped { count: 60 });
}

#[test]
fn an_arc_of_a_sink_is_a_sink() {
    let kept = Arc::new(Kept::default());
    let sink: Arc<dyn DenialSink> = Arc::clone(&kept) as Arc<dyn DenialSink>;
    let clock = HandClock::default();
    let cap = CappedSink::with_clock(sink, &clock);
    cap.denied(path_denial(1));
    assert_eq!(kept.denials(), vec![path_denial(1)]);
}

#[test]
fn denials_print_as_tagged_json() {
    let json = serde_json::to_string(&path_denial(3)).unwrap();
    assert_eq!(
        json,
        r#"{"kind":"path","path":"/outside/3","access":"read","class":"outside"}"#
    );
    let host = Denial::Host {
        host: "example.com".to_owned(),
        port: 443,
        why: HostRefusal::NotAllowed,
        shared_hosting: false,
    };
    assert_eq!(
        serde_json::to_string(&host).unwrap(),
        r#"{"kind":"host","host":"example.com","port":443,"why":"not_allowed","shared_hosting":false}"#
    );
    assert_eq!(
        serde_json::to_string(&Denial::Dropped { count: 2 }).unwrap(),
        r#"{"kind":"dropped","count":2}"#
    );
}
