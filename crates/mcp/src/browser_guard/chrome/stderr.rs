//! The browser's stderr: read to its end, so the browser never blocks on a full pipe. A line
//! saying Chrome's sandbox cannot start ([`sandbox::failure`]) is reported at once, and the start
//! refused even if Chrome would go on; its last lines are reported once it closes stderr (it
//! exited), for a start that fails.
//!
//! The reader runs for the browser's whole life, not just its start: the answer on the pipe and
//! the stderr line race, so a failure printed just before (or any time after) the answer reaches
//! [`Later`], whose hook stops the browser and refuses it (`relay.rs`). The reader only reads
//! (it never blocks on anything but the browser's stderr, which ends when the guard kills
//! Chrome's process group) and forwards nothing of what it reads: the hook gets only the fixed
//! words of [`sandbox::FAILURES`].

use std::collections::VecDeque;
use std::io::BufReader;
use std::sync::{Arc, Mutex, PoisonError, mpsc};

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

/// What is told of a sandbox failure once the start is over: the hook the session sets after the
/// start, or the failure kept until it is set (a line read after the start's last look).
#[derive(Default)]
pub(super) struct Later(Mutex<LaterState>);

#[derive(Default)]
struct LaterState {
    said: Option<&'static str>,
    tell: Option<Hook>,
}

/// Called once with the fixed words of the sandbox failure.
pub(super) type Hook = Box<dyn FnOnce(&'static str) + Send>;

impl Later {
    /// The reader saw a sandbox failure: calls the hook if set, else keeps the failure for it.
    fn failed(&self, said: &'static str) {
        let tell = {
            let mut state = self.lock();
            state.said = Some(said);
            state.tell.take()
        };
        if let Some(tell) = tell {
            tell(said);
        }
    }

    /// Sets the hook; calls it at once if the reader saw a failure already.
    pub(super) fn set(&self, tell: Hook) {
        let mut state = self.lock();
        match state.said {
            Some(said) => {
                drop(state);
                tell(said);
            }
            None => state.tell = Some(tell),
        }
    }

    /// Drops the hook, uncalled (the browser is being stopped).
    pub(super) fn clear(&self) {
        drop(self.lock().tell.take());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LaterState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Reads the browser's stderr to its end, reporting the first sandbox failure as it comes (to the
/// start's `seen` and to `later`), then reports its last lines.
pub(super) fn read_stderr(stderr: ChildStderr, seen: &mpsc::Sender<Seen>, later: &Arc<Later>) {
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
            later.failed(said);
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
