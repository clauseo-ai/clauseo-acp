//! The native crash handler: a fault the OS raises appends its code to the record desktop
//! opened at start (`<data_dir>/crash/native`, through `clauseo-counts`), then the handler that
//! was there before runs, so the process ends as it would have. Everything the handler does is
//! safe in a signal handler: atomic loads and one `write` on a file already open.
//!
//! What counts as evidence of a crash, and nothing else: on macOS and Linux, SIGSEGV, SIGBUS,
//! SIGILL and SIGFPE raised by the kernel for a fault (a `kill` sending one is passed on and not
//! recorded), and SIGABRT, which `abort` raises (on Linux, one another process sent is passed
//! on); on Windows, an exception no handler took (the unhandled-exception filter). A user's kill
//! (SIGTERM, SIGKILL, Task Manager), a power loss or a Windows shutdown raises none of these.
//! Rust's panic abort raises SIGABRT on macOS and Linux after the panic was recorded, so the
//! record stays the panic's alone; on Windows it fails fast, past every filter.

use std::fs::File;
use std::io::Write as _;
use std::sync::OnceLock;

use clauseo_counts::{Crashes, NATIVE_LINE_MAX, native_line};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

#[cfg(unix)]
use unix as os;
#[cfg(windows)]
use win as os;

/// What the handler reads: the crash reports and the record, set once before it is installed.
struct Native {
    crashes: Crashes,
    file: File,
}

static NATIVE: OnceLock<Native> = OnceLock::new();

/// Installs the handler, appending to `file`. Returns whether it was installed.
pub(super) fn install(crashes: Crashes, file: File) -> bool {
    if NATIVE.set(Native { crashes, file }).is_err() {
        return false;
    }
    install_os()
}

#[cfg(any(unix, windows))]
fn install_os() -> bool {
    os::install()
}

#[cfg(not(any(unix, windows)))]
fn install_os() -> bool {
    false
}

/// A crash with `code`, from the handler: one line to the record, the first time only and only
/// while recording. Async-signal-safe: atomics, a stack buffer and `write`.
fn on_fault(code: u32) {
    let Some(native) = NATIVE.get() else {
        return;
    };
    if !native.crashes.begin_crash() {
        return;
    }
    let mut buf = [0_u8; NATIVE_LINE_MAX];
    let line = native_line(code, native.crashes.launched(), &mut buf);
    let mut file = &native.file;
    // Nothing can be done about a failed write while crashing.
    let _crashing = file.write_all(line);
}
