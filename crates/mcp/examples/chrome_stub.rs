//! A stand-in for Chrome in the guard's tests (`tests/guard/`). It prints Chrome's endpoint line,
//! `DevTools listening on ws://127.0.0.1:<port>/devtools/browser/stub-1`, for the port in its
//! `--remote-debugging-port=<port>` on stderr, writes its pid to `chrome.pid` beside itself and,
//! on Unix, its `--user-data-dir` to `profile-of-<parent pid>` there (the guard that started
//! it), and waits to be killed. A file `chrome-mode` beside it changes that: `wrong-port` prints
//! port 47120 instead, `silent` prints nothing, `exit` exits 3 before printing, `listen-exit`
//! prints the endpoint and exits 3 (when, below), `sandbox-exit:<line>` prints `<line>` (one of
//! Chrome's sandbox failures) and exits 1 before the endpoint, `sandbox-stay:<line>` prints it
//! and waits. Each start also adds a line to `chrome-launches` beside it. Tests copy it into
//! their own folder, so each has its own mode and pid file. Cargo builds it for the tests; it is
//! never shipped.
//!
//! Before it prints the endpoint it starts a renderer, a copy of itself with `--type=renderer`
//! and `--stub-sandboxed`, which the guard's check after Chrome's start counts as sandboxed under
//! the tests' `CLAUSEO_TEST_GUARD_SANDBOX=stub` (`src/browser_guard/chrome/postcheck.rs`); the
//! renderer exits when its stdin, which the stand-in holds, closes. Mode `unsandboxed-renderer`
//! starts it without `--stub-sandboxed`. Mode `crash-child` starts it with `--stub-crash` too,
//! after which it exits about 1.5 s later with code 0xC0000005 (an access violation's), and the
//! stand-in never prints its endpoint (Windows' silent start failure, `postcheck/crashed.rs`).
//! `exit:<code>` (decimal, or hex after `0x`) exits at once with that code, before printing:
//! `exit:0xC0000005` is Windows' crash at start in the agent's job. In `listen-exit` the
//! stand-in exits once agent-browser (the echo stub) has made its log `received.jsonl` beside
//! it, at most 10 s after the endpoint, so the guard's check sees it alive first; it writes
//! `chrome-exiting` beside it just before. In `late-unsandboxed` it listens with its sandboxed
//! renderer, and once agent-browser has made its log it starts a second renderer without
//! `--stub-sandboxed` (a child that turns up unsandboxed after the check at the start). A stand-in
//! started with `--no-sandbox` (the guard's `CLAUSEO_TEST_GUARD_CHROME_UNSANDBOXED` hook, a
//! `test-support` build only) starts its renderer unsandboxed, as Chrome would.

use std::io::Write;
use std::path::{Path, PathBuf};

fn main() -> std::io::Result<()> {
    if std::env::args().any(|arg| arg == "--stub-crash") {
        crash_later();
        std::process::exit(0xC000_0005_u32.cast_signed());
    }
    if std::env::args().any(|arg| arg == "--type=renderer") {
        // The renderer: until the stand-in that holds its stdin is gone.
        drop(std::io::Read::read_to_end(
            &mut std::io::stdin(),
            &mut Vec::new(),
        ));
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    let dir = exe.parent().map_or_else(PathBuf::new, PathBuf::from);
    let mode = std::fs::read_to_string(dir.join("chrome-mode")).unwrap_or_default();
    let port = std::env::args()
        .find_map(|arg| {
            arg.strip_prefix("--remote-debugging-port=")
                .map(str::to_owned)
        })
        .unwrap_or_default();
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
    if let Some(code) = mode.trim().strip_prefix("exit:") {
        let code = code
            .strip_prefix("0x")
            .map_or_else(|| code.parse(), |hex| u32::from_str_radix(hex, 16))
            .unwrap_or(1);
        std::process::exit(code.cast_signed());
    }
    if mode.trim() == "crash-child" {
        let _renderer = renderer(&exe, &["--stub-sandboxed", "--stub-crash"])?;
        wait_forever();
    }
    let port = match mode.trim() {
        "exit" => std::process::exit(3),
        "silent" => None,
        "wrong-port" => Some("47120".to_owned()),
        _ => Some(port),
    };
    let exit_after = mode.trim() == "listen-exit";
    let unsandboxed =
        mode.trim() == "unsandboxed-renderer" || std::env::args().any(|arg| arg == "--no-sandbox");
    // Held until the stand-in exits, when the renderer's stdin closes.
    let _renderer = match port {
        Some(_) if unsandboxed => Some(renderer(&exe, &[])?),
        Some(_) => Some(renderer(&exe, &["--stub-sandboxed"])?),
        None => None,
    };
    if let Some(port) = port {
        let mut err = std::io::stderr().lock();
        writeln!(err, "[stub] starting")?;
        writeln!(
            err,
            "DevTools listening on ws://127.0.0.1:{port}/devtools/browser/stub-1"
        )?;
        err.flush()?;
    }
    if exit_after {
        wait_for_agent(&dir);
        std::process::exit(3);
    }
    if mode.trim() == "late-unsandboxed" {
        wait_for_agent(&dir);
        let _late = renderer(&exe, &[])?;
        wait_forever();
    }
    wait_forever();
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

/// Starts the renderer with `args` after `--type=renderer`.
#[expect(
    clippy::disallowed_methods,
    reason = "the test stub starts its stand-in renderer, and waits for agent-browser's log"
)]
fn renderer(exe: &Path, args: &[&str]) -> std::io::Result<std::process::Child> {
    std::process::Command::new(exe)
        .arg("--type=renderer")
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
}

/// Returns once agent-browser's log is in `dir`, at most 10 s from now, writing
/// `chrome-exiting` there for the exit that follows.
#[expect(
    clippy::disallowed_methods,
    reason = "the test stub polls for agent-browser's log and marks its exit"
)]
fn wait_for_agent(dir: &Path) {
    for _ in 0..2_000 {
        if dir.join("received.jsonl").exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    drop(std::fs::write(dir.join("chrome-exiting"), ""));
}

/// The crashing renderer's run: 1.5 s, after which it exits with an access violation's code.
#[expect(
    clippy::disallowed_methods,
    reason = "the test stub's renderer runs a while before it crashes"
)]
fn crash_later() {
    std::thread::sleep(std::time::Duration::from_millis(1_500));
}

/// Blocks until the process is killed.
fn wait_forever() -> ! {
    loop {
        std::thread::park();
    }
}
