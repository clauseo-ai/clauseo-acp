//! The panic hook: a panic's message, location and thread go straight to `host.log`, written
//! on a helper thread that the hook waits for at most [`PANIC_WAIT`], then the previous hook
//! runs. The location is `file:line` of the code, never matter content.

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

/// The panic's line: its message (escaped onto one line), its location and its thread.
pub(super) fn line(info: &PanicHookInfo<'_>) -> String {
    let message = info.payload_as_str().unwrap_or("(no message)");
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

fn append(path: &Path, line: &str) {
    if let Ok(mut file) = write::open_append(path) {
        let _best_effort = file.write_all(line.as_bytes());
    }
}
