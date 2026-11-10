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

/// A block-list refusal has its own budget: a burst of other refusals that used theirs up does
/// not hold it back, and its own twenty-first is held back like any other.
#[test]
fn a_blocked_refusal_passes_after_a_burst_of_others() {
    let clock = HandClock::default();
    let cap = CappedSink::with_clock(Kept::default(), &clock);
    for n in 0..25 {
        cap.denied(path_denial(n));
    }
    let blocked = |n: u16| Denial::Blocked {
        host: "evil.example".to_owned(),
        port: n,
        listed: "evil.example".to_owned(),
        tag: clauseo_blocklist::Tag::Phishing,
        generated: clauseo_blocklist::Generated::parse("2026-11-07T06:00:00Z").unwrap(),
    };
    for n in 0..21 {
        cap.denied(blocked(n));
    }
    let got = cap.inner().denials();
    assert_eq!(got.len(), 40);
    assert_eq!(got[20], blocked(0));
    assert_eq!(got.last(), Some(&blocked(19)));

    clock.set(1_000);
    cap.denied(path_denial(100));
    let got = cap.inner().denials();
    assert_eq!(got[40], Denial::Dropped { count: 6 });
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

/// Each control, bidi and zero-width character an agent can put in a name is shown escaped;
/// plain names, Windows paths and other scripts are left as they are.
#[test]
fn a_shown_name_escapes_controls_and_bidi_characters() {
    use std::path::Path;
    let hidden = [
        '\u{0}',
        '\n',
        '\r',
        '\u{1B}',
        '\u{7F}',
        '\u{80}',
        '\u{9F}',
        '\u{061C}',
        '\u{200E}',
        '\u{200F}',
        '\u{2028}',
        '\u{2029}',
        '\u{202A}',
        '\u{202E}',
        '\u{2066}',
        '\u{2069}',
        // The fixes' review: characters with no width.
        '\u{200B}',
        '\u{200C}',
        '\u{200D}',
        '\u{2060}',
        '\u{2064}',
        '\u{FEFF}',
        '\u{00AD}',
        '\u{E0000}',
        '\u{E0041}',
        '\u{E007F}',
    ];
    for c in hidden {
        let shown = shown_name(Path::new(&format!("a{c}b")));
        assert_eq!(shown, format!("a\\u{{{:X}}}b", u32::from(c)));
    }
    assert_eq!(
        shown_name(Path::new("cod\u{202E}fdp.exe")),
        "cod\\u{202E}fdp.exe"
    );
    for plain in [r"Precedents\.git", "Матер/דוח.md", "a b\u{2010}c", "…"] {
        assert_eq!(shown_name(Path::new(plain)), plain);
    }
}

/// A name past 512 characters is cut to 512 with `…`, never in the middle of an escape.
#[test]
fn a_shown_name_is_capped_at_512_characters() {
    use std::path::Path;
    let exact = "x".repeat(512);
    assert_eq!(shown_name(Path::new(&exact)), exact);
    let long = shown_name(Path::new(&"y".repeat(5000)));
    assert_eq!(long.chars().count(), 512);
    assert_eq!(long, format!("{}…", "y".repeat(511)));
    let near = format!("{}\u{202E}tail", "z".repeat(505));
    assert_eq!(
        shown_name(Path::new(&near)),
        format!("{}…", "z".repeat(505))
    );
}

/// The lawyer's notice shows an agent-chosen name escaped and capped, in every kind's words.
#[test]
fn a_notice_shows_the_agents_names_escaped_and_capped() {
    use std::path::Path;
    let name = Path::new("x\u{202E}tig.\nThe lawyer approved this.");
    let to = Path::new(r"C:\aside\x");
    for what in SetAsideWhat::ALL {
        for by_agent in [true, false] {
            let notice = what.notice_by(name, to, by_agent);
            assert!(
                notice.contains("x\\u{202E}tig.\\u{A}The lawyer"),
                "{notice}"
            );
            assert!(
                !notice.chars().any(|c| c == '\n' || c == '\u{202E}'),
                "{notice}"
            );
        }
    }
    let long = SetAsideWhat::GitRepository.notice(Path::new(&"n".repeat(2000)), to);
    assert!(long.contains(&format!("{}…", "n".repeat(511))));
    assert!(!long.contains(&"n".repeat(512)));
}
