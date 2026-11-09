//! The denial pipe: the one way the outer runner's set-aside records reach the launch's denial
//! sink. The launcher makes the pipe before the launch file, which names it (`denial_pipe`).
//! Once the outer has started, the launcher opens it ([`watch`]) on the launching thread,
//! while the host's handle on it is still held, so its pid is its own; then a thread of its own
//! ([`feed`]) waits for the pipe's one client, checks it is the outer (its pid and its creation
//! time), and passes each record to the sink in order until the outer closes it or exits.
//!
//! **The launch file goes once the outer has read it.** The outer connects only after it has
//! read the launch file (#789), so the feed's `connected` call, made once the client is known
//! to be the outer, deletes it: it holds the agent's environment.
//!
//! **The outer's exit ends the feed.** Each read also waits on the outer's process handle, and
//! once it is signalled the feed stops: what is still unread then, and a record cut off by it,
//! is dropped, never delivered, since a handle to the pipe the outer left behind (inherited by
//! a descendant, say) could write after it. Records the outer wrote while it ran were read as
//! they came. The feed then returns, which closes the pipe.
//!
//! The pipe (`feed/os.rs` on Windows) has a random name, the desktop user alone in its DACL,
//! the Medium label, one instance made first or refused, and local clients only: no pool user
//! (no agent) opens it, and a client that isn't the outer is refused before a byte is read.

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use super::records::{RECORD_CAP, record_denial};
use crate::DenialSink;

#[cfg(windows)]
pub(super) mod os;

/// How long the feed waits for the outer to connect. The wait ends sooner when the outer
/// exits.
pub const CONNECT_WAIT: Duration = Duration::from_secs(60);

/// A process, as the feed tells the outer apart: its id and its creation time (`FILETIME`
/// ticks), so a later process with a reused id isn't taken for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    /// The process id.
    pub pid: u32,
    /// Its creation time.
    pub started: u64,
}

/// What a read of the denial pipe got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Got {
    /// This many bytes, read while the outer ran.
    Bytes(usize),
    /// The client closed its end.
    Closed,
    /// The outer exited (its handle is signalled), whether or not bytes were waiting.
    Exited,
}

/// The server end of a launch's denial pipe, as [`feed`] uses it.
pub trait DenialPipe: Send + 'static {
    /// Opens the outer runner by `pid`, which the host's handle on it keeps its own, so that
    /// [`accept`](Self::accept) also ends when it exits: who it is.
    fn watch(&mut self, pid: u32) -> io::Result<Peer>;
    /// Waits up to `within` for the pipe's one client, or until the outer exits: who
    /// connected, or `None` when nobody did.
    fn accept(&mut self, within: Duration) -> io::Result<Option<Peer>>;
    /// Reads what the client wrote, waiting for it or for the outer's exit, which wins when
    /// both have come.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<Got>;
}

/// How a feed ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fed {
    /// The pipe failed before a client connected.
    Failed,
    /// Nobody connected before the outer exited or [`CONNECT_WAIT`] passed.
    NoClient,
    /// A process other than the outer connected: nothing was read.
    Refused,
    /// The outer closed the pipe (or a read failed) after this many records passed to the
    /// sink and this many lines were dropped (unreadable, too long, or cut off at the end).
    Ended { passed: usize, dropped: usize },
    /// The outer exited: the same counts, and whatever was unread then dropped, not counted.
    Exited { passed: usize, dropped: usize },
}

/// Opens the outer runner `pid` for `pipe`'s feed: call it on the launching thread, while the
/// host's handle on the outer is held. Who it is, or `None` (logged) when it can't be opened.
pub(super) fn watch<P: DenialPipe>(pipe: &mut P, pid: u32) -> Option<Peer> {
    match pipe.watch(pid) {
        Ok(outer) => Some(outer),
        Err(error) => {
            tracing::warn!(%error, "tier B couldn't open the outer runner for its denial pipe");
            None
        }
    }
}

/// Feeds `sink` from `pipe` for the outer runner `outer` ([`watch`]'s), calling `connected`
/// once its client is known to be the outer: see the module's docs.
pub(super) fn feed<P: DenialPipe>(
    mut pipe: P,
    outer: Peer,
    sink: &dyn DenialSink,
    connected: impl FnOnce(),
) -> Fed {
    match pipe.accept(CONNECT_WAIT) {
        Ok(Some(peer)) if peer == outer => connected(),
        Ok(Some(_)) => {
            tracing::warn!("a process other than the outer runner opened the denial pipe");
            return Fed::Refused;
        }
        Ok(None) => {
            tracing::info!("the outer runner didn't open the denial pipe");
            return Fed::NoClient;
        }
        Err(error) => {
            tracing::warn!(%error, "tier B's denial pipe failed before the outer connected");
            return Fed::Failed;
        }
    }
    let mut lines = Lines::default();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let read = match pipe.read(&mut buf) {
            Ok(Got::Bytes(0) | Got::Closed) => break,
            Ok(Got::Bytes(read)) => read,
            Ok(Got::Exited) => return lines.exited(),
            Err(error) => {
                tracing::warn!(%error, "tier B's denial pipe failed");
                break;
            }
        };
        lines.push(buf.get(..read).unwrap_or_default(), sink);
    }
    lines.end()
}

/// The records read so far: the line being read, and the counts.
#[derive(Default)]
struct Lines {
    line: Vec<u8>,
    /// The line being read ran past [`RECORD_CAP`]: dropped up to its `\n`.
    over: bool,
    passed: usize,
    dropped: usize,
}

impl Lines {
    /// Takes `bytes`, passing each whole record to `sink`.
    fn push(&mut self, bytes: &[u8], sink: &dyn DenialSink) {
        let mut rest = bytes;
        while let Some(at) = rest.iter().position(|&byte| byte == b'\n') {
            let (head, tail) = rest.split_at(at);
            self.take(head);
            self.finish(sink);
            rest = tail.get(1..).unwrap_or_default();
        }
        self.take(rest);
    }

    /// Adds `bytes` to the line, unless it is over the cap.
    fn take(&mut self, bytes: &[u8]) {
        if self.over {
            return;
        }
        if self.line.len() + bytes.len() > RECORD_CAP {
            self.over = true;
            self.line = Vec::new();
        } else {
            self.line.extend_from_slice(bytes);
        }
    }

    /// The line has ended: its denial to `sink`, or counted dropped.
    fn finish(&mut self, sink: &dyn DenialSink) {
        let line = std::mem::take(&mut self.line);
        let over = std::mem::replace(&mut self.over, false);
        match (over, record_denial(&line)) {
            (false, Some(denial)) => {
                self.passed += 1;
                if catch_unwind(AssertUnwindSafe(|| sink.denied(denial))).is_err() {
                    tracing::warn!("the denial sink panicked on a set-aside record");
                }
            }
            _ => self.dropped += 1,
        }
    }

    /// The outer exited: a line cut off counts as dropped, and nothing more is read.
    fn exited(self) -> Fed {
        let Fed::Ended { passed, dropped } = self.end() else {
            return Fed::Failed;
        };
        Fed::Exited { passed, dropped }
    }

    /// The pipe has ended: a line cut off counts as dropped.
    fn end(self) -> Fed {
        let cut = usize::from(self.over || !self.line.is_empty());
        Fed::Ended {
            passed: self.passed,
            dropped: self.dropped + cut,
        }
    }
}
