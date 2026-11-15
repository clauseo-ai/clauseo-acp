//! A stand-in for Chrome in the guard's tests (`tests/guard/`). Started with
//! `--remote-debugging-pipe` (route 2), it reads CDP over its `DevTools` pipe, fds 3 and 4 on Unix
//! and the handles in `--remote-debugging-io-pipes=<read>,<write>` on Windows, each message ended
//! by a NUL, and answers every command `{"id":<id>,"result":{"product":"Stub/1"}}`. It prints
//! `[stub] starting` on stderr, writes its pid to `chrome.pid` beside itself and, on Unix, its
//! `--user-data-dir` to `profile-of-<parent pid>` there (the guard that started it), and runs
//! until it is killed or its pipe ends; without the switch (a test's orphan Chrome) it never
//! touches fds 3 and 4 and runs until it is killed. A file `chrome-mode` beside it changes that:
//! `silent` reads but never answers, `exit` exits 3 at once, `answer-exit` answers the first
//! command and exits 3, `sandbox-exit:<line>` prints `<line>` (one of Chrome's sandbox failures)
//! and exits 1 before it answers, `sandbox-stay:<line>` prints it and waits, never answering,
//! `sandbox-late:<line>` answers as usual and prints it [`LATE`] after its first answer.
//! Each start also adds a line to `chrome-launches` beside it. Tests copy it into their own
//! folder, so each has its own mode and pid file. Cargo builds it for the tests; it is never
//! shipped.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// How long after its first answer `sandbox-late` prints its line: past the guard's start, once
/// agent-browser has answered the test's first call.
const LATE: Duration = Duration::from_secs(1);

fn main() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().map_or_else(PathBuf::new, PathBuf::from);
    let mode = std::fs::read_to_string(dir.join("chrome-mode")).unwrap_or_default();
    let mode = mode.trim();
    record(&dir)?;
    if let Some((kind @ ("sandbox-exit" | "sandbox-stay"), line)) = mode.split_once(':') {
        let mut err = std::io::stderr().lock();
        writeln!(err, "[stub] starting")?;
        writeln!(err, "{}", line.trim_end())?;
        err.flush()?;
        drop(err);
        if kind == "sandbox-exit" {
            std::process::exit(1);
        }
        wait_forever();
    }
    if mode == "exit" {
        std::process::exit(3);
    }
    {
        let mut err = std::io::stderr().lock();
        writeln!(err, "[stub] starting")?;
        err.flush()?;
    }
    // Started without a pipe (a test's orphan Chrome), it just runs until it is killed.
    let piped = std::env::args().any(|arg| arg == "--remote-debugging-pipe");
    let Some(Ok((read, mut write))) = piped.then(pipe::open) else {
        wait_forever();
    };
    let mut late = mode
        .strip_prefix("sandbox-late:")
        .map(|line| line.trim_end().to_owned());
    let mut read = BufReader::new(read);
    loop {
        let mut message = Vec::new();
        let Ok(got) = read.read_until(0, &mut message) else {
            wait_forever();
        };
        if got == 0 || message.last() != Some(&0) {
            // The guard closed its end: a browser ends too.
            return Ok(());
        }
        message.pop();
        if mode == "silent" {
            continue;
        }
        let id = serde_json::from_slice::<serde_json::Value>(&message)
            .ok()
            .and_then(|command| command.get("id").cloned())
            .unwrap_or_default();
        let answer = serde_json::json!({"id": id, "result": {"product": "Stub/1"}});
        let mut bytes = answer.to_string().into_bytes();
        bytes.push(0);
        write.write_all(&bytes)?;
        write.flush()?;
        if let Some(line) = late.take() {
            print_later(line);
        }
        if mode == "answer-exit" {
            std::process::exit(3);
        }
    }
}

/// Prints `line` on stderr [`LATE`] from now, on a thread of its own, so the answers go on.
fn print_later(line: String) {
    let (keep, wait) = mpsc::channel::<()>();
    drop(std::thread::spawn(move || {
        // Nothing is ever sent: the wait just takes `LATE`.
        match wait.recv_timeout(LATE) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        drop(keep);
        let mut err = std::io::stderr().lock();
        drop(writeln!(err, "{line}"));
        drop(err.flush());
    }));
}

/// Writes `chrome.pid`, a line more in `chrome-launches` and, on Unix,
/// `profile-of-<parent pid>` into `dir`.
#[expect(
    clippy::disallowed_methods,
    reason = "the test stub records its pid and profile for the test's checks"
)]
fn record(dir: &Path) -> std::io::Result<()> {
    std::fs::write(dir.join("chrome.pid"), std::process::id().to_string())?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("chrome-launches"))?
        .write_all(b"launched\n")?;
    #[cfg(unix)]
    {
        let profile = std::env::args()
            .find_map(|arg| arg.strip_prefix("--user-data-dir=").map(str::to_owned))
            .unwrap_or_default();
        let guard = std::os::unix::process::parent_id();
        std::fs::write(dir.join(format!("profile-of-{guard}")), profile)?;
    }
    Ok(())
}

/// Blocks until the process is killed.
fn wait_forever() -> ! {
    loop {
        std::thread::park();
    }
}

/// The stub's ends of its `DevTools` pipe: (read, write).
#[expect(
    unsafe_code,
    reason = "the pipe's ends are inherited raw fds or handles, which only unsafe code adopts"
)]
mod pipe {
    use std::fs::File;

    /// Fds 3 (read) and 4 (write), Chromium's `kReadFD` and `kWriteFD`; an error if either is
    /// not open (a stub started without the pipe), rather than a wait on a fd that is no pipe.
    #[cfg(unix)]
    pub(super) fn open() -> std::io::Result<(File, File)> {
        use std::os::fd::FromRawFd;
        // SAFETY: F_GETFD only reads a fd's flags, and fails with EBADF on one not open.
        let open = |fd| unsafe { nix::libc::fcntl(fd, nix::libc::F_GETFD) } != -1;
        if !open(3) || !open(4) {
            return Err(std::io::Error::other(
                "fds 3 and 4 are not open: no DevTools pipe",
            ));
        }
        // SAFETY: the guard puts the pipe's ends at fds 3 and 4 before exec, both open as just
        // checked, and nothing else in this process opens or owns them.
        let (read, write) = unsafe { (File::from_raw_fd(3), File::from_raw_fd(4)) };
        Ok((read, write))
    }

    /// The handles in `--remote-debugging-io-pipes=<read>,<write>`, decimal.
    #[cfg(windows)]
    pub(super) fn open() -> std::io::Result<(File, File)> {
        use std::os::windows::io::FromRawHandle;
        let missing =
            || std::io::Error::other("no valid --remote-debugging-io-pipes=<read>,<write>");
        let value = std::env::args()
            .find_map(|arg| {
                arg.strip_prefix("--remote-debugging-io-pipes=")
                    .map(str::to_owned)
            })
            .ok_or_else(missing)?;
        let (read, write) = value.split_once(',').ok_or_else(missing)?;
        let handle = |text: &str| {
            text.parse::<usize>()
                .map(std::ptr::without_provenance_mut::<std::ffi::c_void>)
                .map_err(|error| std::io::Error::other(format!("{}: {error}", missing())))
        };
        let (read, write) = (handle(read)?, handle(write)?);
        // SAFETY: the guard made both handles inheritable for this process and named them in
        // the switch; nothing else in this process owns them.
        Ok(unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) })
    }
}
