//! The host's log file (decision record 0047): `host.log` in [`Locations::log_dir`], warn and
//! error from Clauseo's own crates (`clauseo_*` targets; other crates' events never) plus the
//! host's lifecycle lines ([`LIFECYCLE`]), each line its UTC RFC 3339 time, level, target,
//! message and fields. A deleted log folder is made again at the next write.
//!
//! - A caller formats its line and hands it to a bounded channel ([`CHANNEL_LINES`]) with
//!   `try_send`; a full channel drops the line and counts it, so no thread ever waits on the
//!   disk. One `clauseo-log` thread writes ([`writer`]), putting `dropped N lines` before the
//!   next line it writes.
//! - `host.log` rotates at [`CAP`] into `host.1.log` and `host.2.log`: three files at most.
//!   A failed rotation (Windows: the file held open elsewhere) stops writing at the cap, and a
//!   failed write (a full disk, an I/O error) drops its lines; both count what they drop and
//!   leave the disk alone for [`writer::BACKOFF`] before they try again.
//! - Ids, codes and kinds only, never matter content: a site logs a matter, thread or turn id,
//!   an error through [`Cause`], an exit code, an agent id; never a path, a file name, a prompt,
//!   agent text, a title or a settings value.
//! - The panic hook ([`panic`]) writes each panic's location and thread, and its message only
//!   when that is a `&'static str` (code text), then runs the hook that was set before.
//! - [`LogGuard`]'s drop waits at most [`FLUSH_WAIT`] for the queued lines.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use clauseo_settings::Locations;
use tracing::{Dispatch, Level, Metadata};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::Writer as TimeWriter;
use tracing_subscriber::fmt::time::{FormatTime, SystemTime};
use tracing_subscriber::layer::SubscriberExt;

use crate::clock::{Clock, SystemClock};
use crate::write;

mod cause;
mod panic;
mod writer;

#[cfg(test)]
mod tests;

pub(crate) use cause::Cause;
pub(crate) use writer::{FileSink, Sink, Writer};

/// The target of the host's lifecycle lines, the only info lines the file keeps: the host's
/// start and stop, an agent's launch and end (with its exit code), a matter's open and close.
pub const LIFECYCLE: &str = "clauseo_host::lifecycle";

/// The current file's name in the log folder.
pub(crate) const LOG_FILE: &str = "host.log";

/// The size at which `host.log` rotates: 5 MB, so the three files hold 15 MB at most.
pub(crate) const CAP: u64 = 5 * 1024 * 1024;

/// The lines the channel holds before callers drop theirs.
pub(crate) const CHANNEL_LINES: usize = 1024;

/// The longest line kept; a longer one is cut there, ending in a newline.
pub(crate) const LINE_MAX: usize = 8 * 1024;

/// How long [`LogGuard`]'s drop waits for the queued lines.
pub(crate) const FLUSH_WAIT: Duration = Duration::from_millis(500);

/// What callers and the writer share: the counts, and how far the writer got.
pub(crate) struct Shared {
    /// Lines dropped and not yet reported in a `dropped N lines` line.
    pub(crate) dropped: AtomicU64,
    /// Lines a caller put on the channel.
    sent: AtomicU64,
    /// Lines the writer took off it (written or dropped).
    taken: Mutex<u64>,
    progress: Condvar,
}

impl Shared {
    fn new() -> Self {
        Self {
            dropped: AtomicU64::new(0),
            sent: AtomicU64::new(0),
            taken: Mutex::new(0),
            progress: Condvar::new(),
        }
    }

    /// The writer took `lines` more.
    pub(crate) fn handled(&self, lines: u64) {
        if let Ok(mut taken) = self.taken.lock() {
            *taken += lines;
        }
        self.progress.notify_all();
    }
}

/// Keeps the log going; its drop waits at most [`FLUSH_WAIT`] for the lines queued so far. A
/// guard from a failed [`install`] holds nothing.
#[must_use = "the drop flushes the log at exit"]
pub struct LogGuard {
    shared: Option<Arc<Shared>>,
}

impl LogGuard {
    fn none() -> Self {
        Self { shared: None }
    }

    /// Whether a log file is being written.
    pub fn is_active(&self) -> bool {
        self.shared.is_some()
    }

    /// The lines dropped and not yet named by a written `dropped N lines` line.
    pub fn dropped(&self) -> u64 {
        self.shared
            .as_ref()
            .map_or(0, |shared| shared.dropped.load(Ordering::Relaxed))
    }

    /// Waits at most [`FLUSH_WAIT`] for the lines queued so far to be written or dropped: for
    /// an exit that skips drops (`std::process::exit`).
    pub fn flush(&self) {
        let Some(shared) = self.shared.as_ref() else {
            return;
        };
        let target = shared.sent.load(Ordering::Acquire);
        if let Ok(taken) = shared.taken.lock() {
            let _flushed_or_timed_out =
                shared
                    .progress
                    .wait_timeout_while(taken, FLUSH_WAIT, |taken| *taken < target);
        }
    }
}

impl Drop for LogGuard {
    fn drop(&mut self) {
        self.flush();
    }
}

/// Starts the log in `locations.log_dir()` as the process's global subscriber, with the panic
/// hook. Never fails and never panics: when the folder cannot be made, the thread cannot start
/// or a global subscriber is already set, the guard holds nothing and nothing is logged.
pub fn install(locations: &Locations) -> LogGuard {
    let dir = locations.log_dir();
    if write::create_dir_all(&dir).is_err() {
        return LogGuard::none();
    }
    let Some((dispatch, guard)) = build(FileSink::new(dir.clone()), CAP, monotonic()) else {
        return LogGuard::none();
    };
    if tracing::dispatcher::set_global_default(dispatch).is_err() {
        return LogGuard::none();
    }
    panic::install(dir);
    tracing::info!(target: LIFECYCLE, "the log started");
    guard
}

/// The host's monotonic clock, for the writer's backoff.
fn monotonic() -> Box<dyn Fn() -> Duration + Send> {
    Box::new(|| SystemClock.now().mono)
}

/// THE SINK SEAM: the subscriber writing to `sink` with rotation at `cap`, its writer thread
/// started, and its guard; `None` when the thread does not start.
pub(crate) fn build<S: Sink>(
    sink: S,
    cap: u64,
    now: Box<dyn Fn() -> Duration + Send>,
) -> Option<(Dispatch, LogGuard)> {
    let shared = Arc::new(Shared::new());
    let (lines, taken): (SyncSender<Vec<u8>>, Receiver<Vec<u8>>) = sync_channel(CHANNEL_LINES);
    let writer = Writer::new(sink, cap, Arc::clone(&shared), now);
    std::thread::Builder::new()
        .name("clauseo-log".to_owned())
        .spawn(move || writer.run(&taken))
        .ok()?;
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(Lines {
            lines,
            shared: Arc::clone(&shared),
        })
        .with_filter(filter_fn(kept));
    let subscriber = tracing_subscriber::registry().with(layer);
    let guard = LogGuard {
        shared: Some(shared),
    };
    Some((Dispatch::new(subscriber), guard))
}

/// What the file keeps: warn and error from `clauseo_*` targets, and info from their
/// lifecycle targets. Other crates' events never go in: a dependency's error can hold a
/// request's whole params (agent-client-protocol logs its errors' data at warn), so prompt or
/// file text.
fn kept(meta: &Metadata<'_>) -> bool {
    kept_target(*meta.level(), meta.target())
}

/// [`kept`] for one level and target.
fn kept_target(level: Level, target: &str) -> bool {
    target.starts_with("clauseo_")
        && (level <= Level::WARN || (level == Level::INFO && is_lifecycle(target)))
}

/// A `clauseo_*` crate's lifecycle target, such as [`LIFECYCLE`].
fn is_lifecycle(target: &str) -> bool {
    target.starts_with("clauseo_") && target.ends_with("::lifecycle")
}

/// The time at the start of a line, as the formatter writes it: UTC, RFC 3339.
pub(crate) fn stamp() -> String {
    let mut text = String::new();
    let _never_fails = SystemTime.format_time(&mut TimeWriter::new(&mut text));
    text
}

/// Where `line` is cut: its end, or the last char boundary at or before [`LINE_MAX`] (a
/// UTF-8 continuation byte is never the first byte cut away).
fn line_end(line: &[u8]) -> usize {
    if line.len() <= LINE_MAX {
        return line.len();
    }
    let mut end = LINE_MAX;
    while end > 0
        && line
            .get(end)
            .is_some_and(|byte| byte & 0b1100_0000 == 0b1000_0000)
    {
        end -= 1;
    }
    end
}

/// The formatter's writer: each formatted line goes on the channel or is counted as dropped.
#[derive(Clone)]
struct Lines {
    lines: SyncSender<Vec<u8>>,
    shared: Arc<Shared>,
}

impl<'a> MakeWriter<'a> for Lines {
    type Writer = Line<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        Line(self)
    }
}

/// One line: the formatter writes it whole with one `write_all`.
struct Line<'a>(&'a Lines);

impl io::Write for Line<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut line = buf.get(..line_end(buf)).unwrap_or(buf).to_vec();
        if line.len() < buf.len() {
            line.push(b'\n');
        }
        match self.0.lines.try_send(line) {
            Ok(()) => {
                self.0.shared.sent.fetch_add(1, Ordering::Release);
            }
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.0.shared.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
