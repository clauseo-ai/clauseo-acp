//! The panic hook: a panic's location, thread and (when it is code text) message go straight to
//! `host.log`, written on a helper thread that the hook waits for at most [`PANIC_WAIT`], then
//! the previous hook runs. The location is `file:line` of the code, never matter content.

use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use super::{LOG_FILE, stamp};
use crate::write;

/// How long a panic waits for its line to reach the disk.
const PANIC_WAIT: Duration = Duration::from_millis(250);

/// Puts the hook in front of the one already set, which still runs after it.
pub(super) fn install(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_line(&dir, &line(info));
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

/// Appends `line` to `host.log` in `dir`, best effort: on a helper thread so a hung disk
/// costs the panic [`PANIC_WAIT`] at most, or here when no thread starts.
fn write_line(dir: &Path, line: &str) {
    let path = dir.join(LOG_FILE);
    let (done, wait) = mpsc::channel::<()>();
    let spawned = std::thread::Builder::new()
        .name("clauseo-log-panic".to_owned())
        .spawn({
            let path = path.clone();
            let line = line.to_owned();
            move || {
                append(&path, &line);
                let _waiter_gone = done.send(());
            }
        });
    match spawned {
        Ok(_detached) => {
            let _timed_out = wait.recv_timeout(PANIC_WAIT);
        }
        Err(_) => append(&path, line),
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
