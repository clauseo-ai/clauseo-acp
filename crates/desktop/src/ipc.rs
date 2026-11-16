//! One host.log line per call of a desktop command, so a launch's calls can be laid out against
//! the click's lines from the log alone (on Windows each call holds one of the page's six
//! requests to `http://ipc.localhost` until it answers). Each line names the command and gives:
//!
//! - `at_ms`: when its body started, on the chooser's clock (`chooser/trace.rs`, ms since launch);
//! - `took_ms`: how long the body ran until it answered;
//! - `waited_ms`: from Tauri handing the call over ([`stamped`]) to the body's start, which is
//!   Tauri's async runtime getting to it; -1 when no hand-over was stamped for it;
//! - `running`: the calls whose bodies were running as it started, itself included;
//! - `outcome`: `ok`, `error` or `dropped` (the body never finished).
//!
//! Never an argument, a path or an error's text. Only the first [`LINE_CAP`] calls of a run are
//! logged, and one line says when the rest are not. `ack` calls ([`UNLOGGED`]) are never logged
//! and never count toward the cap.

use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use tauri::Runtime;
use tauri::ipc::Invoke;

use crate::chooser::trace::{at_ms, since};

/// The target of these lines: host.log keeps info only from `clauseo_*::lifecycle` targets
/// (`clauseo_host::log_file`).
pub(crate) const TARGET: &str = "clauseo_desktop::ipc::lifecycle";

/// How many calls a run logs: a launch and its first clicks, never a flood.
pub(crate) const LINE_CAP: u32 = 300;

/// The command whose calls are neither logged nor counted toward [`LINE_CAP`]: the page's
/// acknowledgement of each batch of events it read, many a second while an answer streams, which
/// filled the cap within a minute on the release dry run.
pub(crate) const UNLOGGED: &str = "ack";

/// How many hand-overs wait for their body at most; past it the oldest is forgotten.
const STAMPS_KEPT: usize = 64;

/// The app's ledger.
static LEDGER: Ledger = Ledger::new(LINE_CAP);

/// `handler` (the app's `generate_handler!`), stamping each call's hand-over first.
pub(crate) fn stamped<R: Runtime>(
    handler: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke| {
        LEDGER.received(invoke.message.command());
        handler(invoke)
    }
}

/// Runs the body of `command`, logging its line once it answers.
pub(crate) async fn timed<T, E>(
    command: &'static str,
    body: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    LEDGER.timed(command, body).await
}

/// The calls running and logged, and the hand-overs not yet started.
pub(crate) struct Ledger {
    cap: u32,
    logged: AtomicU32,
    running: AtomicU32,
    /// Each hand-over not yet matched by its body's start, oldest first.
    stamps: Mutex<Vec<(String, Instant)>>,
}

impl Ledger {
    /// A ledger that logs the first `cap` calls.
    pub(crate) const fn new(cap: u32) -> Self {
        Self {
            cap,
            logged: AtomicU32::new(0),
            running: AtomicU32::new(0),
            stamps: Mutex::new(Vec::new()),
        }
    }

    /// Tauri handed over a call of `command`; nothing once the cap is reached, or for an
    /// [`UNLOGGED`] call.
    pub(crate) fn received(&self, command: &str) {
        self.received_at(command, crate::open::now());
    }

    /// [`Ledger::received`] at `at`.
    fn received_at(&self, command: &str, at: Instant) {
        if command == UNLOGGED || self.logged.load(Ordering::Relaxed) >= self.cap {
            return;
        }
        let mut stamps = crate::bridge::lock(&self.stamps);
        if stamps.len() >= STAMPS_KEPT {
            stamps.remove(0);
        }
        stamps.push((command.to_owned(), at));
    }

    /// The oldest hand-over of `command` not yet started, taken.
    fn take_stamp(&self, command: &str) -> Option<Instant> {
        let mut stamps = crate::bridge::lock(&self.stamps);
        let place = stamps.iter().position(|(name, _)| name == command)?;
        Some(stamps.remove(place).1)
    }

    /// Runs `body` as a call of `command`, and logs its line.
    pub(crate) async fn timed<T, E>(
        &self,
        command: &'static str,
        body: impl Future<Output = Result<T, E>>,
    ) -> Result<T, E> {
        let mut call = Call::start(self, command);
        let answer = body.await;
        call.outcome = if answer.is_ok() { "ok" } else { "error" };
        answer
    }

    /// Logs `call`'s line, or the line that says the cap is reached, or nothing past it; nothing
    /// for an [`UNLOGGED`] call, which leaves the count as it was.
    fn log(&self, call: &Call<'_>) {
        if call.command == UNLOGGED {
            return;
        }
        let number = self.logged.fetch_add(1, Ordering::Relaxed);
        if number < self.cap {
            tracing::info!(
                target: TARGET,
                command = call.command,
                at_ms = call.at_ms,
                took_ms = since(call.began),
                waited_ms = call.waited_ms,
                running = call.running,
                outcome = call.outcome,
                "ipc call"
            );
        } else if number == self.cap {
            tracing::info!(target: TARGET, cap = self.cap, "ipc calls past the cap go unlogged");
        }
    }
}

/// One call while its body runs; dropped, it logs its line.
struct Call<'a> {
    ledger: &'a Ledger,
    command: &'static str,
    at_ms: u64,
    began: Instant,
    waited_ms: i64,
    running: u32,
    outcome: &'static str,
}

impl<'a> Call<'a> {
    fn start(ledger: &'a Ledger, command: &'static str) -> Self {
        let waited_ms = ledger
            .take_stamp(command)
            .map_or(-1, |at| i64::try_from(since(at)).unwrap_or(i64::MAX));
        let running = ledger
            .running
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        Self {
            ledger,
            command,
            at_ms: at_ms(),
            began: crate::open::now(),
            waited_ms,
            running,
            outcome: "dropped",
        }
    }
}

impl Drop for Call<'_> {
    fn drop(&mut self) {
        self.ledger.running.fetch_sub(1, Ordering::Relaxed);
        self.ledger.log(self);
    }
}

#[cfg(test)]
mod tests;
