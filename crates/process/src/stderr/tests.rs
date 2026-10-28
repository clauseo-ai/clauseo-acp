use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::Path;

use super::*;

/// A ring that read `bytes` the way the pump does, in reads of at most [`READ`] bytes.
fn ring_with(bytes: &[u8]) -> Mutex<Ring> {
    let mut ring = Ring::new();
    for read in bytes.chunks(READ) {
        ring.push(read);
    }
    Mutex::new(ring)
}

fn no_rules() -> Redactor {
    Redactor::new(None, &BTreeMap::new())
}

fn env(pairs: &[(&str, &str)]) -> BTreeMap<OsString, OsString> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

/// `count` numbered lines of exactly 40 bytes, `\n` included.
fn numbered_lines(count: usize) -> String {
    let mut lines = String::new();
    for n in 1..=count {
        writeln!(lines, "line {n:05} of the agent's stderr.......").unwrap();
    }
    lines
}

/// Every line `on_line` gets from `reads`, fed one read at a time, then the end of the pipe.
fn split(reads: &[&[u8]]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut splitter = LineSplitter::default();
    let mut on_line = |line: &str| lines.push(line.to_owned());
    for read in reads {
        splitter.feed(read, &mut on_line);
    }
    splitter.finish(&mut on_line);
    lines
}

#[test]
fn tail_at_most_4_kib_whole_lines() {
    let stderr = numbered_lines(200);
    assert_eq!(stderr.len(), 8000);
    let tail = tail(&ring_with(stderr.as_bytes()), &no_rules());
    assert!(tail.len() <= TAIL, "{}", tail.len());
    // 4096 bytes cut a line; the tail starts on the next one and keeps every line after it.
    assert_eq!(tail.len(), 102 * 40);
    assert!(tail.starts_with("line 00099 "), "{tail:?}");
    assert!(tail.ends_with("line 00200 of the agent's stderr.......\n"));
    assert!(stderr.ends_with(&tail));
}

#[test]
fn short_stderr_comes_whole() {
    let stderr = numbered_lines(3);
    assert_eq!(tail(&ring_with(stderr.as_bytes()), &no_rules()), stderr);
    assert_eq!(tail(&ring_with(b""), &no_rules()), "");
    let exactly = "x".repeat(TAIL - 1) + "\n";
    assert_eq!(tail(&ring_with(exactly.as_bytes()), &no_rules()), exactly);
}

#[test]
fn redacts_home_and_tokens_before_cutting() {
    let secret = "a1b2c3d4".repeat(12);
    let rules = Redactor::new(
        Some(Path::new("/Users/lawyer")),
        &env(&[
            ("ANTHROPIC_API_KEY", &secret),
            ("github_token", "fake-token-value"),
            ("DB_PASSWORD", "short"),
            ("PATH", "/usr/local/bin:/usr/bin"),
        ]),
    );
    let mut stderr = format!("the key is {secret} here\n");
    stderr.push_str("open /Users/lawyer/matters/brief.docx failed\n");
    stderr.push_str("token fake-token-value, password short, path /usr/local/bin:/usr/bin\n");
    stderr.push_str("Authorization: Bearer eyJhbGciOi.J9-x_y/z+w= sent\n");
    stderr.push_str("key=sk-ant-api03-XYZ_abc-123 and task-list and risk-free\n");
    // Filler so that a cut of the raw text 4096 bytes from the end would land in the secret.
    let filler = 4096 - (stderr.len() - stderr.find(&secret).unwrap() - secret.len() / 2);
    stderr.push_str(&"f".repeat(filler - 1));
    stderr.push('\n');
    let raw_cut = &stderr[stderr.len() - TAIL..];
    assert!(secret.contains(&raw_cut[..8]), "{raw_cut:?}");

    let tail = tail(&ring_with(stderr.as_bytes()), &rules);
    assert!(tail.starts_with("the key is [redacted] here\n"), "{tail:?}");
    assert!(tail.contains("open ~/matters/brief.docx failed"));
    assert!(tail.contains("token [redacted], password short, path /usr/local/bin:/usr/bin"));
    assert!(tail.contains("Authorization: Bearer [redacted] sent"));
    assert!(tail.contains("key=[redacted] and task-list and risk-free"));
    for start in 0..=secret.len() - 8 {
        assert!(
            !tail.contains(&secret[start..start + 8]),
            "a piece of the secret survived"
        );
    }
}

#[test]
fn redaction_rules_stand_alone() {
    let rules = Redactor::new(None, &env(&[("SECRET", "abcdefgh"), ("KEY", "abcdefg")]));
    assert_eq!(rules.redact("abcdefgh abcdefg"), "[redacted] abcdefg");
    assert_eq!(rules.redact("sk-"), "[redacted]");
    assert_eq!(
        rules.redact("(sk-1) x_sk-2 2sk-3 -sk-4"),
        "([redacted]) x_sk-2 2sk-3 -[redacted]"
    );
    assert_eq!(
        rules.redact("Bearer  two spaces, Bearer"),
        "Bearer  two spaces, Bearer"
    );
    assert_eq!(rules.redact("Bearer a\"b"), "Bearer [redacted]\"b");
    // A secret that holds another goes whole.
    let rules = Redactor::new(
        None,
        &env(&[("A_TOKEN", "12345678"), ("B_TOKEN", "12345678-extra")]),
    );
    assert_eq!(
        rules.redact("12345678-extra 12345678"),
        "[redacted] [redacted]"
    );
    // An empty home is no rule.
    let rules = Redactor::new(Some(Path::new("")), &BTreeMap::new());
    assert_eq!(rules.redact("a/b"), "a/b");
}

#[test]
fn utf8_boundary_kept() {
    // One line of three-byte chars, longer than the tail and with no `\n` after the cut.
    let line = "日本語".repeat(600);
    let tail = tail(&ring_with(line.as_bytes()), &no_rules());
    assert!(
        tail.len() <= TAIL && tail.len() > TAIL - 3,
        "{}",
        tail.len()
    );
    assert!(line.ends_with(&tail));
    assert!(!tail.contains('\u{FFFD}'));
    // Bytes that are not UTF-8 become U+FFFD.
    let tail = super::tail(&ring_with(b"bad \xff byte\n"), &no_rules());
    assert_eq!(tail, "bad \u{FFFD} byte\n");
}

#[test]
fn redaction_window_widens_when_short() {
    let secret = "s".repeat(1000);
    let rules = Redactor::new(None, &env(&[("API_KEY", &secret)]));
    let mut stderr = numbered_lines(1000);
    // The last 16 KiB are secrets alone, which redaction shrinks under 4096 bytes.
    for _ in 0..17 {
        stderr.push_str(&secret);
        stderr.push('\n');
    }
    let tail = tail(&ring_with(stderr.as_bytes()), &rules);
    assert!(tail.len() <= TAIL, "{}", tail.len());
    assert!(
        tail.len() > TAIL - 40,
        "the window did not widen: {}",
        tail.len()
    );
    assert!(tail.ends_with(&"[redacted]\n".repeat(17)));
    assert!(tail.contains("line 01000 of the agent's stderr.......\n[redacted]\n"));
    assert!(tail.starts_with("line "), "{tail:?}");
    assert!(!tail.contains("ss"));
}

#[test]
fn ring_wraps_and_keeps_last_256_kib() {
    let stderr: Vec<u8> = (0..300 * 1024_u32)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect();
    let ring = ring_with(&stderr);
    let ring = ring.lock().unwrap();
    assert_eq!(ring.len(), RING);
    assert!(!ring.holds_start());
    let mut out = Vec::new();
    ring.last(RING, &mut out);
    assert_eq!(out, stderr[stderr.len() - RING..]);
    ring.last(10, &mut out);
    assert_eq!(out, stderr[stderr.len() - 10..]);
    ring.last(RING + 1, &mut out);
    assert_eq!(out.len(), RING);
}

#[test]
fn ring_keeps_the_last_bytes_of_an_oversized_push() {
    let mut ring = Ring::new();
    ring.push(b"start\n");
    let big: Vec<u8> = (0..u32::try_from(RING).unwrap() + 7)
        .map(|i| u8::try_from(i % 13).unwrap())
        .collect();
    ring.push(&big);
    let mut out = Vec::new();
    ring.last(RING, &mut out);
    assert_eq!(out, big[7..]);
    assert!(!ring.holds_start());
}

#[test]
fn ring_fills_exactly_to_its_end() {
    let mut ring = Ring::new();
    ring.push(&vec![b'a'; RING - 1]);
    assert!(ring.holds_start());
    assert_eq!(ring.len(), RING - 1);
    ring.push(b"b");
    assert!(!ring.holds_start());
    assert_eq!(ring.len(), RING);
    let mut out = Vec::new();
    ring.last(2, &mut out);
    assert_eq!(out, b"ab");
}

#[test]
fn wrapped_ring_tail_opens_on_a_whole_line() {
    // Short lines after a wrap: the tail is the whole window, cut after its first `\n`.
    let mut stderr = "x".repeat(RING);
    stderr.push_str("half a line\nwhole\n");
    let tail = tail(&ring_with(stderr.as_bytes()), &no_rules());
    assert_eq!(tail, "whole\n");
}

#[test]
fn lines_split_across_reads() {
    let lines = split(&[b"one\ntw", b"o\r\nthr", b"ee", b"\n\nlast"]);
    assert_eq!(lines, ["one", "two", "three", "", "last"]);
    assert_eq!(split(&[]), Vec::<String>::new());
    assert_eq!(split(&[b"\r\n"]), [""]);
}

#[test]
fn a_line_longer_than_16_kib_comes_in_pieces() {
    let mut read = vec![b'a'; MAX_LINE * 2 + 5];
    read.push(b'\n');
    let reads: Vec<&[u8]> = read.chunks(1000).collect();
    let lens: Vec<usize> = split(&reads).iter().map(String::len).collect();
    assert_eq!(lens, [MAX_LINE, MAX_LINE, 5]);
}

#[test]
fn a_line_of_exactly_16_kib_comes_whole() {
    let line = vec![b'a'; MAX_LINE];
    let lens: Vec<usize> = split(&[&line, b"\nnext\n"])
        .iter()
        .map(String::len)
        .collect();
    assert_eq!(lens, [MAX_LINE, 4]);
    assert_eq!(split(&[&line]).len(), 1);
}

#[test]
fn a_line_that_is_not_utf8_comes_lossy() {
    assert_eq!(split(&[b"\xc3", b"\xbcber \xff\n"]), ["über \u{FFFD}"]);
}

#[test]
fn keep_last_cuts_at_a_char_and_after_a_newline() {
    assert_eq!(keep_last("abc", 3), "abc");
    assert_eq!(keep_last("ab\ncd", 3), "cd");
    assert_eq!(keep_last("éé", 3), "é");
    assert_eq!(keep_last("a\nbcdef", 6), "bcdef");
}
