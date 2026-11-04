//! The runner: one `<node> <script> args…` child of a sign-in, status or detection, started
//! through the caller's `Launcher`, with its whole environment built by `super::env`.
//!
//! It follows the pattern of `node_version` (`node.rs`) and `Npm::run` (`install/npm.rs`):
//! stdout read to its end, stderr driven through its pump, and the exit awaited, all joined;
//! that work raced against the cancel signal and the run's time limit through the `Timer`. On
//! a cancel or the limit the handle is dropped, which kills the child's tree.
//!
//! stdout and stderr go into a 64 KB ring in memory ([`Ring`]) that is never shown or logged,
//! and each line is offered to a callback as it arrives (the flows look for a device code, a
//! refusal or a cancel there). `process` keeps its own stderr ring for `ExitReport::stderr_tail`;
//! nothing here reads it.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Launcher, Timer};
use futures::future::{Either, join3, select};
use futures::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::cancel::Cancel;
use crate::signin::PasteCode;

/// The ring's size: the last 64 KB of output.
pub(crate) const RING_BYTES: usize = 64 * 1024;

/// A line longer than this comes to the callback in pieces of this size, as `process` does for
/// stderr.
const LINE_MAX: usize = 16 * 1024;

/// One run. Its `Debug` names the environment's variables and never prints a value (a proxy's
/// password, an inherited key).
#[derive(Clone)]
pub(crate) struct RunSpec {
    /// Node: `Installed::node`.
    pub(crate) node: PathBuf,
    /// The script Node runs: the adapter's (`--cli`, Claude) or `codex.js` (Codex).
    pub(crate) script: PathBuf,
    /// The arguments after the script.
    pub(crate) args: Vec<OsString>,
    /// The child's whole environment, from `super::env::run_env`.
    pub(crate) env: BTreeMap<OsString, OsString>,
    /// The folder it runs in.
    pub(crate) cwd: PathBuf,
    /// Past this the run is killed: 5 s for a status, 10 minutes for a login.
    pub(crate) limit: Duration,
}

impl std::fmt::Debug for RunSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunSpec")
            .field("node", &self.node)
            .field("script", &self.script)
            .field("args", &self.args)
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .field("cwd", &self.cwd)
            .field("limit", &self.limit)
            .finish()
    }
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended {
    /// It exited by itself: its code, or `None` after a signal.
    Exited(Option<i32>),
    /// The limit passed; it was killed.
    TimedOut,
    /// The cancel signal was set; it was killed.
    Cancelled,
}

/// A finished run.
#[derive(Debug)]
pub(crate) struct Ran {
    /// How it ended.
    pub(crate) ended: Ended,
    /// The last [`RING_BYTES`] of its output, stdout and stderr interleaved by line.
    pub(crate) output: Ring,
}

/// The last [`RING_BYTES`] bytes of a run's output. Never shown or logged: its `Debug` prints
/// only its length.
#[derive(Default)]
pub(crate) struct Ring {
    bytes: VecDeque<u8>,
}

impl Ring {
    /// Adds `bytes`, dropping the oldest past [`RING_BYTES`].
    fn push(&mut self, bytes: &[u8]) {
        let keep = bytes.len().min(RING_BYTES);
        let tail = bytes.get(bytes.len() - keep..).unwrap_or_default();
        let over = (self.bytes.len() + tail.len()).saturating_sub(RING_BYTES);
        self.bytes.drain(..over.min(self.bytes.len()));
        self.bytes.extend(tail);
    }

    /// Its length in bytes.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Its text, with U+FFFD for bytes that are not UTF-8 (a cut at the front included).
    pub(crate) fn text(&self) -> String {
        let (front, back) = self.bytes.as_slices();
        let mut all = Vec::with_capacity(self.bytes.len());
        all.extend_from_slice(front);
        all.extend_from_slice(back);
        String::from_utf8_lossy(&all).into_owned()
    }
}

impl std::fmt::Debug for Ring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ring({} bytes)", self.bytes.len())
    }
}

/// Splits stdout into lines as `process` splits stderr: without the `\n` or a `\r` before it,
/// U+FFFD for bytes that are not UTF-8, [`LINE_MAX`] pieces, and a last line with no `\n` at the
/// end.
#[derive(Default)]
struct Lines {
    pending: Vec<u8>,
}

impl Lines {
    /// Feeds `bytes`, calling `on_line` for each line they complete.
    fn feed(&mut self, bytes: &[u8], on_line: &mut impl FnMut(&str)) {
        for &byte in bytes {
            if byte == b'\n' {
                self.emit(on_line);
            } else {
                self.pending.push(byte);
                if self.pending.len() >= LINE_MAX {
                    self.emit(on_line);
                }
            }
        }
    }

    /// Calls `on_line` with what is left, if anything.
    fn finish(&mut self, on_line: &mut impl FnMut(&str)) {
        if !self.pending.is_empty() {
            self.emit(on_line);
        }
    }

    fn emit(&mut self, on_line: &mut impl FnMut(&str)) {
        if self.pending.last() == Some(&b'\r') {
            self.pending.pop();
        }
        on_line(&String::from_utf8_lossy(&self.pending));
        self.pending.clear();
    }
}

/// Runs `spec` through `launcher`. Each code from `paste` goes to the child's stdin as
/// [`PasteCode::line`] gives it; with no `paste` the child's stdin is closed at once, and once
/// `paste` closes, so is stdin. `on_line` gets each line of stdout and stderr as it arrives.
/// Ends when the child exits and its output is read, when `cancel` is set, or when
/// `spec.limit` passes; in the last two the child's tree is killed. Err when it could not start.
pub(crate) async fn run<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    spec: RunSpec,
    cancel: &Cancel,
    paste: Option<async_channel::Receiver<PasteCode>>,
    on_line: impl FnMut(&str) + Send,
) -> Result<Ran, clauseo_process::Error> {
    let mut argv = Vec::with_capacity(spec.args.len().saturating_add(1));
    argv.push(spec.script.into_os_string());
    argv.extend(spec.args);
    let launch = LaunchSpec::new(spec.node, argv, spec.env, spec.cwd)?;
    let Launched {
        mut stdin,
        mut stdout,
        stderr,
        mut handle,
        ..
    } = launcher.launch(&launch)?;
    let ring = Mutex::new(Ring::default());
    let on_line = Mutex::new(on_line);
    let offer = |line: &str| {
        (*on_line.lock().unwrap_or_else(PoisonError::into_inner))(line);
    };
    if paste.is_none() {
        drop(stdin.close().await);
    }
    let ended = {
        let output = pin!(async {
            let read = async {
                let mut lines = Lines::default();
                let mut chunk = [0_u8; 8192];
                let mut offer_line = |line: &str| offer(line);
                while let Ok(n @ 1..) = stdout.read(&mut chunk).await {
                    let bytes = chunk.get(..n).unwrap_or_default();
                    ring.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(bytes);
                    lines.feed(bytes, &mut offer_line);
                }
                lines.finish(&mut offer_line);
            };
            let errs = stderr.lines(|line| {
                {
                    let mut ring = ring.lock().unwrap_or_else(PoisonError::into_inner);
                    ring.push(line.as_bytes());
                    ring.push(b"\n");
                }
                offer(line);
            });
            let ((), drained, report) = join3(read, errs, handle.exited()).await;
            drop(drained);
            report.code()
        });
        let feed = pin!(async {
            if let Some(paste) = paste {
                while let Ok(code) = paste.recv().await {
                    let line = code.line();
                    drop(code);
                    // A child that closed its stdin just does not get the code.
                    if stdin.write_all(&line).await.is_ok() {
                        drop(stdin.flush().await);
                    }
                }
                drop(stdin.close().await);
            }
            futures::future::pending::<Option<i32>>().await
        });
        let work = select(output, feed);
        let cancelled = pin!(cancel.cancelled());
        let limit = pin!(timer.after(spec.limit));
        match select(work, select(cancelled, limit)).await {
            Either::Left((done, _)) => Ended::Exited(done.factor_first().0),
            Either::Right((Either::Left(_), _)) => Ended::Cancelled,
            Either::Right((Either::Right(_), _)) => Ended::TimedOut,
        }
    };
    drop(handle);
    let output = ring.into_inner().unwrap_or_else(PoisonError::into_inner);
    Ok(Ran { ended, output })
}

#[cfg(test)]
#[cfg(unix)]
mod tests;
