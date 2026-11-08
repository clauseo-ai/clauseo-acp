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

#[test]
fn display_safe_escapes_controls_and_bidi_and_invisible_formatting_and_keeps_the_rest() {
    assert_eq!(display_safe("a b/ü/日本.txt"), "a b/ü/日本.txt");
    assert_eq!(display_safe("a\nb\tc\u{0}"), r"a\u{000A}b\u{0009}c\u{0000}");
    assert_eq!(
        display_safe("\u{7F}\u{85}\u{9F}"),
        r"\u{007F}\u{0085}\u{009F}"
    );
    assert_eq!(
        display_safe("invoice\u{202E}fdp.exe"),
        r"invoice\u{202E}fdp.exe"
    );
    for c in [
        '\u{200B}', '\u{200F}', '\u{202A}', '\u{2066}', '\u{2069}', '\u{061C}', '\u{FEFF}',
    ] {
        let shown = display_safe(&c.to_string());
        assert_eq!(shown, format!("\\u{{{:04X}}}", u32::from(c)));
    }
    // Just outside the ranges: kept. U+2065 (unassigned, default ignorable) is escaped.
    assert_eq!(display_safe("\u{2010}\u{A0}"), "\u{2010}\u{A0}");
    assert_eq!(display_safe("\u{2065}"), r"\u{2065}");
    assert_eq!(
        display_safe_path(Path::new("C:\\x\u{2067}y")),
        PathBuf::from(r"C:\x\u{2067}y")
    );
}

#[test]
fn the_reporters_pause_passes_the_cap_even_in_a_full_window() {
    let clock = HandClock::default();
    let cap = CappedSink::with_clock(Kept::default(), &clock);
    for n in 0..25 {
        cap.denied(path_denial(n));
    }
    cap.denied(Denial::ReporterPaused { secs: 30 });
    let got = cap.inner().denials();
    assert_eq!(got.len(), 21);
    assert_eq!(got.last(), Some(&Denial::ReporterPaused { secs: 30 }));
}

/// Every code point of `ranges` (both ends included) is escaped.
fn all_escaped(ranges: &[(u32, u32)]) {
    for &(first, last) in ranges {
        for code in first..=last {
            let Some(c) = char::from_u32(code) else {
                continue;
            };
            assert!(is_invisible_or_control(c), "U+{code:04X}");
            assert_eq!(display_safe(&c.to_string()), format!("\\u{{{code:04X}}}"));
        }
    }
}

#[test]
fn every_invisible_or_control_range_is_escaped() {
    all_escaped(&[
        // Cc, Zl, Zp.
        (0x00, 0x1F),
        (0x7F, 0x9F),
        (0x2028, 0x2029),
        // Default ignorable, and Cf among them.
        (0xAD, 0xAD),
        (0x34F, 0x34F),
        (0x61C, 0x61C),
        (0x115F, 0x1160),
        (0x17B4, 0x17B5),
        (0x180B, 0x180F),
        (0x200B, 0x200F),
        (0x202A, 0x202E),
        (0x2060, 0x206F),
        (0x3164, 0x3164),
        (0xFE00, 0xFE0F),
        (0xFEFF, 0xFEFF),
        (0xFFA0, 0xFFA0),
        (0x1BCA0, 0x1BCA3),
        (0x1D173, 0x1D17A),
        (0xE0000, 0xE0FFF),
        // Other Cf: Arabic number signs, the interlinear annotations.
        (0x600, 0x605),
        (0xFFF9, 0xFFFB),
        // Private use.
        (0xE000, 0xF8FF),
        (0xF0000, 0xF0010),
        (0xFFFF0, 0x10FFFD),
        // Noncharacters.
        (0xFDD0, 0xFDEF),
        (0xFFFE, 0xFFFF),
        (0x1FFFE, 0x1FFFF),
        (0x10FFFE, 0x10FFFF),
    ]);
}

#[test]
fn letters_and_marks_of_every_script_and_emoji_are_kept() {
    for name in [
        "दस्तावेज़.docx",
        "Résumé – Ångström naïve.pdf",
        "契約書_第2版.docx",
        "계약서.hwp",
        "عقد.pdf",
        "📄 notes 🧾 🙂.txt",
        "C:\\Users\\Zoë\\x y.txt",
    ] {
        assert_eq!(display_safe(name), name);
    }
}
