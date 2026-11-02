//! The host-wide denial watcher: one `/usr/bin/log stream --style ndjson` whose predicate keeps
//! only the kernel Sandbox's lines that carry a Clauseo tag. It runs while any launcher is
//! registered, maps each line to its launch by the tag, and reports file denials to that
//! launch's sink.
//!
//! The `sender == "Sandbox"` term keeps out lines a jailed process writes itself through
//! `logger` or `os_log`: only the kernel's Sandbox extension sends as `Sandbox`. The tag is the
//! last line of the message, which the kernel appends after the report, so a path holding a
//! newline and another tag cannot claim another launch.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use async_process::{Child, ChildStdout, Command};
use futures::future::{Either, select};
use futures::io::{AsyncBufReadExt, BufReader};
use futures::{StreamExt, pin_mut};

use super::launcher::LaunchSink;
use super::profile::TAG_PREFIX;
use crate::{Access, Clock, Denial, DenialSink, Policy, SystemClock};

/// The unified log's command-line front end.
const LOG: &str = "/usr/bin/log";

/// The predicate: the kernel Sandbox's lines with any Clauseo tag.
const PREDICATE: &str = r#"sender == "Sandbox" AND eventMessage CONTAINS "clauseo-""#;

/// A watcher that fails to start or dies is started again at most this often.
const RESTART_EVERY: Duration = Duration::from_secs(60);

/// How long the first registration waits for `log stream` to say it is filtering, so the
/// first launch's denials are not missed.
const START_WAIT: Duration = Duration::from_secs(2);

/// How often every registered sink's rate cap is ticked.
const TICK: Duration = Duration::from_secs(1);

/// One registered launch.
struct Entry {
    policy: Arc<Policy>,
    sink: Arc<LaunchSink>,
}

/// The watcher's shared state.
#[derive(Default)]
struct State {
    launches: BTreeMap<String, Entry>,
    /// Bumped each time the watcher starts from no registrations; a supervisor whose
    /// generation is stale stops.
    generation: u64,
    /// The running `log stream`; killing it ends its supervisor's read.
    child: Option<Child>,
}

static STATE: Mutex<State> = Mutex::new(State {
    launches: BTreeMap::new(),
    generation: 0,
    child: None,
});

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A launch's place in the watcher; dropping it unregisters the launch, and the last drop
/// stops `log stream`.
#[derive(Debug)]
pub(crate) struct Registration {
    tag: String,
}

impl Drop for Registration {
    fn drop(&mut self) {
        let child = {
            let mut state = state();
            state.launches.remove(&self.tag);
            if state.launches.is_empty() {
                state.child.take()
            } else {
                None
            }
        };
        // Dropping the child kills it (kill_on_drop); its supervisor sees the end and stops.
        drop(child);
    }
}

/// Registers the launch tagged `tag`. The first registration starts the watcher and waits up
/// to two seconds for it to start filtering; a watcher that does not start is logged and
/// tried again later, and the launch goes ahead either way.
pub(crate) fn register(tag: String, policy: Arc<Policy>, sink: Arc<LaunchSink>) -> Registration {
    let first = {
        let mut state = state();
        let first = state.launches.is_empty();
        state.launches.insert(tag.clone(), Entry { policy, sink });
        if first {
            state.generation += 1;
            Some(state.generation)
        } else {
            None
        }
    };
    if let Some(generation) = first {
        let (started, wait) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("clauseo-sandbox-watch".to_owned())
            .spawn(move || supervise(generation, &started));
        match spawned {
            Ok(_) => {
                if wait.recv_timeout(START_WAIT).is_err() {
                    tracing::warn!("the Seatbelt denial watcher did not start in time");
                }
            }
            Err(error) => {
                tracing::error!(%error, "the Seatbelt denial watcher's thread did not start");
            }
        }
    }
    Registration { tag }
}

/// Runs `log stream` for `generation` until no launch is registered, restarting it at most
/// once a minute when it fails to start or ends.
fn supervise(generation: u64, started: &mpsc::Sender<()>) {
    let clock = SystemClock::new();
    let mut last_start: Option<Duration> = None;
    loop {
        if let Some(last) = last_start {
            let wait = (last + RESTART_EVERY).saturating_sub(clock.now());
            async_io::block_on(async_io::Timer::after(wait));
        }
        if !current(generation) {
            return;
        }
        last_start = Some(clock.now());
        match start(generation) {
            Ok(stdout) => {
                read(stdout, started);
                if !current(generation) {
                    return;
                }
                tracing::warn!("the Seatbelt denial watcher ended; restarting it");
            }
            Err(error) => {
                tracing::error!(%error, "the Seatbelt denial watcher did not start");
            }
        }
    }
}

/// Whether `generation` is the running one and a launch is registered.
fn current(generation: u64) -> bool {
    let state = state();
    state.generation == generation && !state.launches.is_empty()
}

/// Starts `log stream` and keeps the child in the state, so the last unregistration kills it.
fn start(generation: u64) -> std::io::Result<ChildStdout> {
    let mut child = Command::new(LOG)
        .args(["stream", "--style", "ndjson", "--predicate", PREDICATE])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("log stream started without its stdout"))?;
    let mut state = state();
    if state.generation == generation && !state.launches.is_empty() {
        state.child = Some(child);
    }
    // Otherwise the child drops here and is killed: no launch is left to watch for.
    Ok(stdout)
}

/// Reads lines until the stream ends, reporting each denial and ticking the sinks once a
/// second. The tick is one interval timer for the whole read, so lines that arrive faster than
/// once a second do not hold it back. The first line (`Filtering the log data using ...`)
/// tells the first registration the watcher is ready.
fn read(stdout: ChildStdout, started: &mpsc::Sender<()>) {
    let mut lines = BufReader::new(stdout).lines();
    let mut ticks = async_io::Timer::interval(TICK);
    async_io::block_on(async {
        loop {
            let next = lines.next();
            let tick = ticks.next();
            pin_mut!(next);
            match select(next, tick).await {
                Either::Left((Some(Ok(line)), _)) => {
                    if !line.starts_with('{') {
                        // The header. A closed receiver (after the first start) is fine.
                        if started.send(()).is_err() {
                            tracing::trace!("the Seatbelt denial watcher restarted");
                        }
                        continue;
                    }
                    deliver(&line);
                }
                Either::Left((Some(Err(error)), _)) => {
                    tracing::warn!(%error, "reading the Seatbelt denial watcher failed");
                    return;
                }
                Either::Left((None, _)) => return,
                Either::Right((_, _)) => tick_all(),
            }
        }
    });
}

/// Ticks every registered sink, outside the lock.
fn tick_all() {
    let sinks: Vec<Arc<LaunchSink>> = state()
        .launches
        .values()
        .map(|entry| Arc::clone(&entry.sink))
        .collect();
    for sink in sinks {
        sink.tick();
    }
}

/// Reports the denial on `line` to its launch's sink, if it is one.
fn deliver(line: &str) {
    let Some(found) = parse(line) else {
        return;
    };
    let entry = {
        let state = state();
        state
            .launches
            .get(&found.tag)
            .map(|entry| (Arc::clone(&entry.policy), Arc::clone(&entry.sink)))
    };
    let Some((policy, sink)) = entry else {
        return;
    };
    report(&found, &policy, &sink);
}

/// Reports `found` to its launch's `sink`: a file denial as [`Denial::Path`] classified by
/// `policy`, the log's fold of repeated file denials as [`Denial::Dropped`]; network and
/// other denials only to the log. A device (`/dev/...`) is never reported: it is nothing the
/// lawyer can allow, and every shell asks for `/dev/tty`, which the jail refuses.
fn report(found: &LogDenial, policy: &Policy, sink: &LaunchSink) {
    if found.target.starts_with("/dev/") {
        tracing::debug!(
            tag = %found.tag,
            process = %found.process,
            operation = %found.operation,
            target = %found.target,
            "the jail refused a device"
        );
        return;
    }
    match access(&found.operation) {
        Some(_) if found.repeats > 0 => {
            // The log's fold of repeats: the first report went out; the rest are a count.
            sink.denied(Denial::Dropped {
                count: found.repeats,
            });
        }
        Some(access) => {
            let path = PathBuf::from(&found.target);
            let class = policy.classify(&path, access);
            sink.denied(Denial::Path {
                path,
                access,
                class,
            });
        }
        None if found.operation.starts_with("network-") => {
            // A direct connection is not a site the lawyer can allow: the proxy is the way out.
            tracing::info!(
                tag = %found.tag,
                process = %found.process,
                pid = found.pid,
                operation = %found.operation,
                target = %found.target,
                "the jail refused a direct connection"
            );
        }
        None => tracing::debug!(
            tag = %found.tag,
            process = %found.process,
            pid = found.pid,
            operation = %found.operation,
            target = %found.target,
            "the jail refused an operation that is not reported"
        ),
    }
}

/// The access a file operation is, if it is one the watcher reports: reading a file's data
/// (or listing a folder), and every write. Metadata and extended-attribute reads are not
/// reported: runtimes probe many paths that way (module lookups, quarantine checks), and a
/// probe is not a read the lawyer can sensibly allow.
fn access(operation: &str) -> Option<Access> {
    if operation == "file-read-data" {
        Some(Access::Read)
    } else if operation.starts_with("file-write-") {
        Some(Access::Write)
    } else {
        None
    }
}

/// One Sandbox denial line, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LogDenial {
    tag: String,
    /// For the log's `<n> duplicate reports for ...` line, `n`; else 0.
    repeats: u32,
    process: String,
    pid: u32,
    operation: String,
    target: String,
}

/// Parses one ndjson line of `log stream`. The message reads
/// `Sandbox: <name>(<pid>) deny(<n>) <operation> <target>`, then a newline and the tag. A line
/// of the form `<n> duplicate reports for Sandbox: ...`, which the log writes when one process
/// repeats a denial, parses with `repeats` set to `n`: the first report already went out, and
/// the repeats reach the sink as one [`Denial::Dropped`].
fn parse(line: &str) -> Option<LogDenial> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let message = value.get("eventMessage")?.as_str()?;
    let (report, tag) = message.rsplit_once('\n')?;
    if !valid_tag(tag) {
        return None;
    }
    let (repeats, report) = repeats(report)?;
    let report = report.strip_prefix("Sandbox: ")?;
    let (who, rest) = report.split_once(") deny(")?;
    let (process, pid) = who.rsplit_once('(')?;
    let pid = pid.parse().ok()?;
    let (_, rest) = rest.split_once(") ")?;
    let (operation, target) = rest.split_once(' ')?;
    Some(LogDenial {
        tag: tag.to_owned(),
        repeats,
        process: process.to_owned(),
        pid,
        operation: operation.to_owned(),
        target: target.to_owned(),
    })
}

/// Splits `<n> duplicate report(s) for <report>` into `n` and the report; any other report
/// comes back whole with 0. `None` for a count too large to hold.
fn repeats(report: &str) -> Option<(u32, &str)> {
    for marker in [" duplicate reports for ", " duplicate report for "] {
        if let Some((n, rest)) = report.split_once(marker)
            && !n.is_empty()
            && n.bytes().all(|b| b.is_ascii_digit())
        {
            return Some((n.parse().ok()?, rest));
        }
    }
    Some((0, report))
}

/// Whether `tag` has the shape the launcher makes.
fn valid_tag(tag: &str) -> bool {
    tag.strip_prefix(TAG_PREFIX).is_some_and(|rest| {
        !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

#[cfg(test)]
mod tests;
