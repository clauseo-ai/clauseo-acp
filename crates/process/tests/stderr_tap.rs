//! The stderr line tap (`SpawnOptions::stderr_tap`), with the agent writing exactly the bytes the
//! test chose: it is this test binary run again (`support/self_agent.rs`), copying its stdin to
//! its stderr. The tap sees each line in order; what `drain` keeps and the exit tail shows does
//! not change.

#[cfg(test)]
#[path = "support/self_agent.rs"]
mod self_agent;

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use async_io::block_on;
use clauseo_process::{Launched, SpawnOptions};
use futures::{AsyncWriteExt, join};

/// The agent the tests here start, not a check: run as one, it copies its stdin to its stderr
/// as it is, then returns. In a normal run it returns at once.
#[test]
fn tap_agent_copies_its_stdin_to_stderr() {
    if !self_agent::is_agent() {
        return;
    }
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .expect("read stdin");
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(&input).expect("write stderr");
    stderr.flush().expect("flush stderr");
}

/// The lines a test's tap collected, shared with the tap.
#[cfg(test)]
type Collected = Arc<Mutex<Vec<String>>>;

/// Options whose tap collects every line into the returned list.
#[cfg(test)]
fn collecting() -> (Collected, SpawnOptions) {
    let seen = Collected::default();
    let sink = Arc::clone(&seen);
    let opts = SpawnOptions::new().stderr_tap(move |line| {
        sink.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_owned());
    });
    (seen, opts)
}

/// What the tap collected, so far.
#[cfg(test)]
fn taken(seen: &Collected) -> Vec<String> {
    seen.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Runs the agent with `input` on its stdin, with `opts`, and drains its stderr. Returns the exit
/// tail, after checking that the agent exited 0 and the drain succeeded.
#[cfg(test)]
fn run(input: &[u8], opts: &SpawnOptions) -> String {
    let Launched {
        mut stdin,
        stdout: _stdout,
        stderr,
        mut handle,
        ..
    } = self_agent::launch(
        "tap_agent_copies_its_stdin_to_stderr",
        opts,
        BTreeMap::new(),
    )
    .expect("launch the agent");
    let (report, pumped) = block_on(async {
        stdin.write_all(input).await.expect("write its stdin");
        stdin.close().await.expect("close its stdin");
        join!(handle.exited(), stderr.drain())
    });
    pumped.expect("drain its stderr");
    assert_eq!(report.code(), Some(0));
    report.stderr_tail()
}

/// The tap sees each line in order, without its `\n` or a `\r` before it, and `drain` keeps every
/// byte, as it does with no tap.
#[test]
fn the_tap_sees_each_line_in_order_and_drain_keeps_all() {
    let input = b"first\nsecond\r\n\nthird line\n";
    let (seen, opts) = collecting();
    let tail = run(input, &opts);
    assert_eq!(taken(&seen), ["first", "second", "", "third line"]);
    assert_eq!(tail.as_bytes(), input);
    assert_eq!(run(input, &SpawnOptions::new()), tail);
}

/// A last line with no `\n` reaches the tap at the end of stderr.
#[test]
fn a_partial_last_line_comes_at_eof() {
    let (seen, opts) = collecting();
    let tail = run(b"one\ntwo", &opts);
    assert_eq!(taken(&seen), ["one", "two"]);
    assert_eq!(tail, "one\ntwo");
}

/// A line that is not UTF-8 comes with U+FFFD for the bad bytes, and the bytes stay as they were
/// in what `drain` keeps.
#[test]
fn a_line_that_is_not_utf8_comes_lossy() {
    let input = b"ok\n\x80bad\xff\n";
    let (seen, opts) = collecting();
    let tail = run(input, &opts);
    assert_eq!(taken(&seen), ["ok", "\u{FFFD}bad\u{FFFD}"]);
    assert_eq!(tail, run(input, &SpawnOptions::new()));
}

/// A line longer than 16 KiB comes in pieces of at most 16 KiB, which join back to the line.
#[test]
fn a_long_line_comes_in_pieces() {
    let line = "x".repeat(40 * 1024);
    let input = format!("{line}\nafter\n");
    let (seen, opts) = collecting();
    let tail = run(input.as_bytes(), &opts);
    let lines = taken(&seen);
    let (pieces, after) = lines.split_at(lines.len() - 1);
    assert!(pieces.len() >= 3, "{} pieces", pieces.len());
    assert!(pieces.iter().all(|piece| piece.len() <= 16 * 1024));
    assert_eq!(pieces.concat(), line);
    assert_eq!(after, ["after"]);
    assert!(tail.ends_with("after\n"), "{tail:.40}");
}

/// A multi-byte character that the 16 KiB cut splits comes lossy in each piece, and nothing
/// panics: the cut is at 16 KiB of the line's bytes, whatever the reads are.
#[test]
fn a_character_split_at_the_16_kib_cut_comes_lossy() {
    let mut line = "a".repeat(16 * 1024 - 1);
    // Two bytes, so the cut falls between them.
    line.push('é');
    line.push_str("bbb");
    let (seen, opts) = collecting();
    let tail = run(format!("{line}\n").as_bytes(), &opts);
    let mut first = "a".repeat(16 * 1024 - 1);
    first.push('\u{FFFD}');
    assert_eq!(taken(&seen), [first, "\u{FFFD}bbb".to_owned()]);
    assert!(tail.ends_with("bbb\n"), "{tail:.40}");
}

/// A tap that panics on its first line does not stop the pump: `drain` returns every byte, and
/// the tap gets no line after the panic.
#[test]
fn a_tap_that_panics_leaves_drain_whole() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let opts = SpawnOptions::new().stderr_tap(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        panic!("the tap fails on its first line");
    });
    let input = b"first\nsecond\nthird\n";
    let tail = run(input, &opts);
    assert_eq!(tail.as_bytes(), input);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
