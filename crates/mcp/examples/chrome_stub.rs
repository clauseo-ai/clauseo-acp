//! A stand-in for Chrome in the guard's tests (`tests/guard/`). It prints Chrome's endpoint line,
//! `DevTools listening on ws://127.0.0.1:<port>/devtools/browser/stub-1`, for the port in its
//! `--remote-debugging-port=<port>` on stderr, writes its pid to `chrome.pid` beside itself and,
//! on Unix, its `--user-data-dir` to `profile-of-<parent pid>` there (the guard that started
//! it), and waits to be killed. A file `chrome-mode` beside it changes that: `wrong-port` prints
//! port 47120 instead, `silent` prints nothing, `exit` exits 3 before printing, `listen-exit`
//! prints the endpoint and exits 3 at once. Tests copy it into their own folder, so each has its
//! own mode and pid file. Cargo builds it for the tests; it is never shipped.

use std::io::Write;
use std::path::{Path, PathBuf};

fn main() -> std::io::Result<()> {
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
    let port = match mode.trim() {
        "exit" => std::process::exit(3),
        "silent" => None,
        "wrong-port" => Some("47120".to_owned()),
        _ => Some(port),
    };
    let exit_after = mode.trim() == "listen-exit";
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
        std::process::exit(3);
    }
    wait_forever();
}

/// Writes `chrome.pid` and, on Unix, `profile-of-<parent pid>` into `dir`.
#[expect(
    clippy::disallowed_methods,
    reason = "the test stub records its pid and profile for the test's checks"
)]
fn record(dir: &Path) -> std::io::Result<()> {
    std::fs::write(dir.join("chrome.pid"), std::process::id().to_string())?;
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
