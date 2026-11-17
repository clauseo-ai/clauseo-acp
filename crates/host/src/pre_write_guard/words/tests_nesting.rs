//! The reader's depth and size caps ([`MAX_NESTING`], [`MAX_NESTED_BYTES`]). Ordinary commands
//! read as before: `shell_writes/tests_before.rs`.

use std::time::Duration;

use crate::clock::{Clock, SystemClock};

use super::{Escape, MAX_NESTED_BYTES, MAX_NESTING, Segments, powershell_words, shell_segments};

/// `open` nested `depth` times around `inside`.
fn nested(open: &str, depth: usize, inside: &str) -> String {
    format!("{}{inside}{}", open.repeat(depth), ")".repeat(depth))
}

fn read(command: &str, limit: usize) -> Segments {
    let mut segments = Segments::new(limit);
    shell_segments(command, Escape::Backtick, &mut segments);
    segments
}

#[test]
fn a_reading_stops_past_the_nesting_cap_and_is_cut() {
    for open in ["$(", "<<$("] {
        let within = read(&nested(open, MAX_NESTING, "x"), usize::MAX);
        assert!(!within.cut(), "{open}");
        for limit in [64, 4096, usize::MAX] {
            let past = read(&nested(open, MAX_NESTING + 1, "x"), limit);
            assert!(past.cut() && past.unsure(), "{open} {limit}");
        }
    }
}

#[test]
fn a_reading_stops_past_the_nested_size_cap_and_is_cut() {
    let body = "x".repeat(MAX_NESTED_BYTES / 4);
    let four = format!("$({body}) $({body}) $({body}) $({body})");
    assert!(!read(&four, usize::MAX).cut());
    let five = format!("{four} $({body})");
    let past = read(&five, usize::MAX);
    assert!(past.cut() && past.unsure());
}

/// How long a reading may take: the reading is linear in the command, at about a microsecond a
/// byte in a test build (debug) and well under a tenth of that optimised.
const BOUND: Duration = Duration::from_secs(if cfg!(debug_assertions) { 10 } else { 1 });

/// `read` run on a 1 MiB stack, and how long it took.
fn on_small_stack<T: Send>(read: impl FnOnce() -> T + Send) -> (T, Duration) {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn_scoped(scope, || {
                let started = SystemClock.now().mono;
                let read = read();
                (read, SystemClock.now().mono.saturating_sub(started))
            })
            .unwrap()
            .join()
            .unwrap()
    })
}

#[test]
fn a_deeply_nested_or_long_command_is_read_quickly_on_a_small_stack() {
    let long = format!("$($({}))", "x".repeat(MAX_NESTED_BYTES));
    let cases = [
        (nested("$(", 20_000, "x"), &[64, 4096, usize::MAX][..]),
        (nested("<<$(", 20_000, "x"), &[64, 4096, usize::MAX][..]),
        (long, &[usize::MAX][..]),
    ];
    for (past, limits) in cases {
        let command = format!("Remove-Item a.txt; {past}");
        for &limit in limits {
            let (cut, took) = on_small_stack(|| read(&command, limit).cut());
            assert!(cut, "{limit}");
            assert!(took < BOUND, "{limit}: {took:?}");
        }
        let (words, took) = on_small_stack(|| powershell_words(&command));
        assert!(words.iter().flatten().any(|word| word == "Remove-Item"));
        assert!(took < BOUND, "words: {took:?}");
    }
}
