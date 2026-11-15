//! The browser's stderr: read to its end, so the browser never blocks on a full pipe. A line
//! saying Chrome's sandbox cannot start ([`sandbox::failure`]) is reported at once, and the start
//! refused even if Chrome would go on; its last lines are reported once it closes stderr (it
//! exited), for a start that fails.

use std::collections::VecDeque;
use std::io::BufReader;
use std::sync::mpsc;

use super::super::ChildStderr;
use super::super::relay::{Line, Overflow, read_line};
use super::sandbox;

/// The longest stderr line kept; longer ones are skipped.
const MAX_STDERR_LINE: usize = 64 * 1024;
/// How many of the browser's last stderr lines a start failure quotes.
const TAIL_LINES: usize = 5;

/// What the browser's stderr reader saw.
pub(super) enum Seen {
    /// The browser closed stderr; its last lines.
    Failed(Vec<String>),
    /// Chrome said its sandbox cannot start: the fixed part of what it said
    /// ([`sandbox::failure`]). Reported once, at the first such line.
    Sandbox(&'static str),
}

/// Reads the browser's stderr to its end, reporting the first sandbox failure as it comes, then
/// reports its last lines.
pub(super) fn read_stderr(stderr: ChildStderr, seen: &mpsc::Sender<Seen>) {
    let mut input = BufReader::new(stderr);
    let mut line = Vec::new();
    let mut tail = VecDeque::new();
    let mut sandbox_failed = false;
    loop {
        match read_line(&mut input, &mut line, MAX_STDERR_LINE, Overflow::Skip) {
            Ok(Line::Whole | Line::Partial) => {}
            Ok(Line::Oversize) => continue,
            Ok(Line::Eof) | Err(_) => break,
        }
        if tail.len() == TAIL_LINES {
            tail.pop_front();
        }
        let text = String::from_utf8_lossy(&line);
        if !sandbox_failed && let Some(said) = sandbox::failure(&text) {
            sandbox_failed = true;
            drop(seen.send(Seen::Sandbox(said)));
        }
        tail.push_back(text.trim().to_owned());
    }
    drop(seen.send(Seen::Failed(tail.into_iter().collect())));
}

/// `; its last stderr lines: ...`, or nothing.
pub(super) fn quote(tail: &[String]) -> String {
    if tail.is_empty() {
        String::new()
    } else {
        format!("; its last stderr lines: {}", tail.join(" | "))
    }
}
