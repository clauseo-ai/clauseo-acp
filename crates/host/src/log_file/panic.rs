//! The panic hook: a panic's location, thread and (when it is code text) message go straight to
//! `host.log`, written on a helper thread that the hook waits for at most [`PANIC_WAIT`], then
//! the previous hook runs. The location is `file:line` of the code, never matter content. While
//! crash reports record (`crate::crash`), the same thread first writes the panic's report, made
//! here, and the hook waits up to [`CRASH_WAIT`].

use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use clauseo_counts::Pending;

use super::{LOG_FILE, stamp};
use crate::write;

/// How long a panic waits for its line to reach the disk.
const PANIC_WAIT: Duration = Duration::from_millis(250);
/// How long it waits when a crash report goes first (a temp file, a sync and a rename).
const CRASH_WAIT: Duration = Duration::from_millis(750);

/// Puts the hook in front of the one already set, which still runs after it.
pub(super) fn install(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let report = crate::crash::on_panic(info);
        write_line(&dir, &line(info), report);
        previous(info);
    }));
}

/// The panic's line: its location, its thread, and its message when that is code text (a
/// `&'static str` payload, such as `panic!("literal")`), escaped onto one line. A formatted
/// message (a `String` payload) can hold content (a slicing panic prints the string, an
/// `expect` text a path), so it is left out.
pub(super) fn line(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    let message = match payload.downcast_ref::<&'static str>() {
        Some(text) => text,
        None if payload.is::<String>() => "message omitted (formatted)",
        None => "(no message)",
    };
    let location = info.location().map_or_else(
        || "unknown".to_owned(),
        |at| format!("{}:{}", at.file(), at.line()),
    );
    let current = std::thread::current();
    let thread = current.name().unwrap_or("unnamed");
    format!(
        "{} ERROR clauseo_host::panic: a thread panicked thread={thread:?} location={location} \
         message={message:?}\n",
        stamp()
    )
}

/// Writes `report` (when there is one), then appends `line` to `host.log` in `dir`, best
/// effort: on a helper thread so a hung disk costs the panic [`PANIC_WAIT`] at most
/// ([`CRASH_WAIT`] with a report), or here when no thread starts.
fn write_line(dir: &Path, line: &str, report: Option<Pending>) {
    let path = dir.join(LOG_FILE);
    let wait_for = if report.is_some() {
        CRASH_WAIT
    } else {
        PANIC_WAIT
    };
    let (done, wait) = mpsc::channel::<()>();
    let report = std::sync::Mutex::new(report);
    let report = std::sync::Arc::new(report);
    let spawned = std::thread::Builder::new()
        .name("clauseo-log-panic".to_owned())
        .spawn({
            let path = path.clone();
            let line = line.to_owned();
            let report = std::sync::Arc::clone(&report);
            move || {
                write_report(&report);
                append(&path, &line);
                let _waiter_gone = done.send(());
            }
        });
    if let Ok(_detached) = spawned {
        let _timed_out = wait.recv_timeout(wait_for);
    } else {
        write_report(&report);
        append(&path, line);
    }
}

/// Writes the report held in `report`, if it is still there.
fn write_report(report: &std::sync::Mutex<Option<Pending>>) {
    let taken = report.lock().map_or(None, |mut held| held.take());
    if let Some(pending) = taken {
        // Best effort: a panic has nowhere to tell a failed write.
        let _best_effort = pending.write();
    }
}

/// Appends `line` to `path`, making its folder again when someone deleted it.
fn append(path: &Path, line: &str) {
    let opened = match write::open_append(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => path
            .parent()
            .map_or(Err(error), write::create_dir_all)
            .and_then(|()| write::open_append(path)),
        opened => opened,
    };
    if let Ok(mut file) = opened {
        let _best_effort = file.write_all(line.as_bytes());
    }
}
