//! What the exit report's stderr tail shows of an agent's stderr, with the agent writing exactly
//! the bytes the test chose: it is this test binary run again (`support/self_agent.rs`), reading
//! its stderr from its stdin.

#[cfg(test)]
#[path = "support/self_agent.rs"]
mod self_agent;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read as _, Write as _};
use std::path::PathBuf;

use async_io::block_on;
use clauseo_process::{Launched, SpawnOptions};
use futures::{AsyncWriteExt, join};

/// The agent the test here starts, not a check: run as one, it reads its stdin to the end,
/// writes what it read to stderr as it is and returns. In a normal run it returns at once.
#[test]
fn agent_copies_its_stdin_to_stderr() {
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

/// The home folder given in `SpawnOptions::home` shows as `~` in the stderr tail.
#[test]
fn home_folder_shows_as_tilde_in_the_tail() {
    let home = "/home/clauseo-test-user";
    let opts = SpawnOptions::new().home(PathBuf::from(home));
    let Launched {
        mut stdin,
        stdout: _stdout,
        stderr,
        mut handle,
        ..
    } = self_agent::launch("agent_copies_its_stdin_to_stderr", &opts, BTreeMap::new()).unwrap();
    let line = format!("cannot read {home}/project/notes.md\n");
    let (report, pumped) = block_on(async {
        stdin.write_all(line.as_bytes()).await.unwrap();
        stdin.close().await.unwrap();
        join!(handle.exited(), stderr.drain())
    });
    pumped.unwrap();
    assert_eq!(report.code(), Some(0));
    assert_eq!(report.stderr_tail(), "cannot read ~/project/notes.md\n");
}

/// The stderr tail of the agent run as `agent_copies_its_stdin_to_stderr`, given `input` on its
/// stdin, with no secret variables and no home folder.
#[cfg(test)]
fn tail_of(input: &[u8]) -> String {
    tail_with_env(input, BTreeMap::new())
}

/// As [`tail_of`], with `env`'s variables in the agent's environment.
#[cfg(test)]
fn tail_with_env(input: &[u8], env: BTreeMap<OsString, OsString>) -> String {
    let Launched {
        mut stdin,
        stdout: _stdout,
        stderr,
        mut handle,
        ..
    } = self_agent::launch(
        "agent_copies_its_stdin_to_stderr",
        &SpawnOptions::new(),
        env,
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

/// A key longer than the 16 KiB the tail redacts first, alone on its line, comes out as
/// `[redacted]`: none of its end is left at the start of the tail (issue 138).
#[test]
fn a_key_longer_than_the_window_leaves_no_end_in_the_tail() {
    let key = format!("sk-{}", "k".repeat(20_000));
    let tail = tail_of(format!("{key}\n").as_bytes());
    assert_eq!(tail, "[redacted]\n");
}

/// Ordinary text after such a key, on the same line, is still in the tail.
#[test]
fn text_after_a_key_longer_than_the_window_is_kept() {
    let key = format!("sk-{}", "k".repeat(20_000));
    let tail = tail_of(format!("{key} then the agent gave up\n").as_bytes());
    assert_eq!(tail, "[redacted] then the agent gave up\n");
}

/// A key longer than the whole 256 KiB ring has no start left to redact. The tail leaves out
/// the key's end and keeps the text after it.
#[test]
fn a_key_longer_than_the_ring_leaves_only_the_text_after_it() {
    let key = format!("sk-{}", "k".repeat(300 * 1024));
    let tail = tail_of(format!("{key} then the agent gave up\n").as_bytes());
    assert_eq!(tail, " then the agent gave up\n");
}

/// One line longer than the window, of chars no key holds, opens mid-line as before: its last
/// whole chars within 4096 bytes, nothing left out.
#[test]
fn a_long_line_of_other_text_still_opens_mid_line() {
    let line = "日本語 ".repeat(2_000) + "\n";
    let tail = tail_of(line.as_bytes());
    let cut = (line.len() - 4_096..line.len())
        .find(|&at| line.is_char_boundary(at))
        .unwrap();
    assert_eq!(tail, line[cut..]);
    assert!(tail.starts_with("語 日本語 "), "{:?}", &tail[..20]);
}

/// A bearer token longer than the window, of chars a key does not hold (`.`, `/`), comes out
/// as `[redacted]`: none of its end is left at the start of the tail.
#[test]
fn a_bearer_token_longer_than_the_window_leaves_no_end_in_the_tail() {
    let token = "eyJ.x/".repeat(4_000);
    let tail = tail_of(format!("Authorization: Bearer {token}\n").as_bytes());
    assert_eq!(tail, "Authorization: Bearer [redacted]\n");
}

/// A secret variable's value may hold any char, spaces too. Written again and again on one line
/// longer than the ring, so no copy's start is left to redact the first one by: the cut-off end
/// of that copy is left out, the whole copies become `[redacted]`, and the text after is kept.
#[test]
fn a_secret_value_longer_than_the_window_leaves_no_end_in_the_tail() {
    let value = "horse; battery staple!".repeat(1_000);
    let env = BTreeMap::from([(OsString::from("AGENT_PASSWORD"), OsString::from(&value))]);
    let line = vec![value.as_str(); 14].join(" ") + " done\n";
    assert!(line.len() > 256 * 1024);
    let tail = tail_with_env(line.as_bytes(), env);
    assert!(tail.starts_with(" [redacted] [redacted]"), "{tail:?}");
    assert_eq!(tail.trim_start_matches(" [redacted]"), " done\n");
}

/// The ring's size (`RING` in `src/stderr.rs`).
const RING: usize = 256 * 1024;

/// The tail when stderr is `x`s, `before`, then exactly the ring's size of `start`, a space,
/// an `sk-` key and ` done\n`: the ring holds `start` on, cut from `before`, so the tail's
/// window starts there, and the key redacts to 10 bytes, so nothing hides that start.
#[cfg(test)]
fn tail_cut_at(before: &[u8], start: &[u8], env: BTreeMap<OsString, OsString>) -> String {
    let end = b" done\n";
    let key = RING - start.len() - " sk-".len() - end.len();
    let mut stderr = b"x".repeat(1_000);
    stderr.extend_from_slice(before);
    stderr.extend_from_slice(start);
    stderr.extend_from_slice(b" sk-");
    stderr.extend(std::iter::repeat_n(b'k', key));
    stderr.extend_from_slice(end);
    tail_with_env(&stderr, env)
}

/// One secret variable.
#[cfg(test)]
fn secret(name: &str, value: &str) -> BTreeMap<OsString, OsString> {
    BTreeMap::from([(OsString::from(name), OsString::from(value))])
}

/// A window that starts inside `Bearer `, or just after it, leaves out the token after it.
#[test]
fn a_cut_bearer_marker_leaves_no_token_in_the_tail() {
    for split in [1, 3, 5, 6] {
        let (before, after) = "Authorization: Bearer ".split_at("Authorization: ".len() + split);
        let start = format!("{after}SECRETTOK123");
        let tail = tail_cut_at(before.as_bytes(), start.as_bytes(), BTreeMap::new());
        assert_eq!(tail, " [redacted] done\n", "cut after {before:?}");
    }
}

/// A window that starts inside a char of a secret value leaves out the value's end: a cut
/// after the first of `ä`'s two bytes, or of `€`'s three.
#[test]
fn a_secret_value_cut_inside_a_char_leaves_no_end_in_the_tail() {
    for value in ["pässwörd: horse battery", "p€sswörd: horse battery"] {
        let (before, start) = value.as_bytes().split_at(2);
        let tail = tail_cut_at(before, start, secret("AGENT_PASSWORD", value));
        assert_eq!(tail, " [redacted] done\n", "{value:?}");
    }
}

/// A value with a `\n`, as in a PEM key, cut so the window opens on the line after it, or
/// starts before that `\n` and opens after it, leaves out the value's last line.
#[test]
fn a_secret_value_cut_at_its_newline_leaves_no_end_in_the_tail() {
    let value = "first line pw\nsecond half secret!";
    for cut in ["first line pw\n".len(), "first li".len()] {
        let (before, start) = value.as_bytes().split_at(cut);
        let tail = tail_cut_at(before, start, secret("AGENT_TOKEN", value));
        assert_eq!(tail, " [redacted] done\n", "cut at {cut}");
    }
}

/// Bytes that are not UTF-8 at stderr's very start still show as U+FFFD: only a window cut
/// from what came before leaves out a char's last bytes.
#[test]
fn a_stray_byte_at_the_start_of_stderr_shows_as_a_replacement_char() {
    let tail = tail_of(b"\x80 starts mid-char\n");
    assert_eq!(tail, "\u{FFFD} starts mid-char\n");
}
