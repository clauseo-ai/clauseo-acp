//! The stderr line tap (`SpawnOptions::stderr_tap`), with the agent writing exactly the bytes the
//! test chose: it is this test binary run again (`support/self_agent.rs`), copying its stdin to
//! its stderr. The tap sees each line in order; what `drain` keeps and the exit tail shows does
//! not change.

#[cfg(test)]
#[path = "support/self_agent.rs"]
mod self_agent;

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
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

/// Runs the agent with `input` on its stdin, with a tap if `tapped`, and drains its stderr. Returns
/// the lines the tap saw and the exit tail.
#[cfg(test)]
fn run(input: &[u8], tapped: bool) -> (Vec<String>, String) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut opts = SpawnOptions::new();
    if tapped {
        let sink = Arc::clone(&seen);
        opts = opts.stderr_tap(move |line| {
            sink.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(line.to_owned());
        });
    }
    let Launched {
        mut stdin,
        stdout: _stdout,
        stderr,
        mut handle,
        ..
    } = self_agent::launch(
        "tap_agent_copies_its_stdin_to_stderr",
        &opts,
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
    let lines = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    (lines, report.stderr_tail())
}

/// The tap sees each line in order, without its `\n` or a `\r` before it, and `drain` keeps every
/// byte, as it does with no tap.
#[test]
fn the_tap_sees_each_line_in_order_and_drain_keeps_all() {
    let input = b"first\nsecond\r\n\nthird line\n";
    let (lines, tail) = run(input, true);
    assert_eq!(lines, ["first", "second", "", "third line"]);
    assert_eq!(tail.as_bytes(), input);
    let (no_lines, untapped) = run(input, false);
    assert_eq!(no_lines, Vec::<String>::new());
    assert_eq!(untapped, tail);
}

/// A last line with no `\n` reaches the tap at the end of stderr.
#[test]
fn a_partial_last_line_comes_at_eof() {
    let (lines, tail) = run(b"one\ntwo", true);
    assert_eq!(lines, ["one", "two"]);
    assert_eq!(tail, "one\ntwo");
}

/// A line that is not UTF-8 comes with U+FFFD for the bad bytes, and the bytes stay as they were
/// in what `drain` keeps.
#[test]
fn a_line_that_is_not_utf8_comes_lossy() {
    let input = b"ok\n\x80bad\xff\n";
    let (lines, tail) = run(input, true);
    assert_eq!(lines, ["ok", "\u{FFFD}bad\u{FFFD}"]);
    let (_, untapped) = run(input, false);
    assert_eq!(tail, untapped);
}

/// A line longer than 16 KiB comes in pieces of at most 16 KiB, which join back to the line.
#[test]
fn a_long_line_comes_in_pieces() {
    let line = "x".repeat(40 * 1024);
    let input = format!("{line}\nafter\n");
    let (lines, tail) = run(input.as_bytes(), true);
    let (pieces, after) = lines.split_at(lines.len() - 1);
    assert!(pieces.len() >= 3, "{} pieces", pieces.len());
    assert!(pieces.iter().all(|piece| piece.len() <= 16 * 1024));
    assert_eq!(pieces.concat(), line);
    assert_eq!(after, ["after"]);
    assert!(tail.ends_with("after\n"), "{tail:.40}");
}
