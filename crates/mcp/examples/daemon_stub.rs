//! A stand-in for agent-browser in the guard's lifecycle tests (`tests/guard/lifecycle.rs`). On
//! Unix, at its first `tools/call` (where agent-browser's CLI starts its daemon), it starts itself
//! again as `daemon_stub --daemon` in a process group of its own, the way agent-browser's daemon
//! leaves the guard's group (`setsid`); that daemon listens on
//! `<AGENT_BROWSER_SOCKET_DIR>/<AGENT_BROWSER_SESSION>.sock` (removing a stale socket file there
//! first, as agent-browser does), writes its pid to `<session>.pid` beside it, as agent-browser's
//! does, and waits to be killed; once it runs, this process also writes the daemon's pid to
//! `daemon-of-<parent pid>.pid` there (the guard that started it). Then, on every OS, it answers
//! each line that has an `id` with an empty result, and exits 0 at the end of its stdin. With the
//! argument `linger` it also starts itself as `daemon_stub --linger` in its own process
//! group, standing in for a CLI call of agent-browser's that outlives it: that process writes its
//! pid to `linger.pid` in the socket folder and waits to be killed. With the argument `late` it
//! starts no daemon at once but `daemon_stub --late` in a process group of its own: a daemon that
//! has left agent-browser's group but not yet bound its socket. That process writes its pid to
//! `late.pid` in the socket folder, waits until a file `late-go` appears there, then serves as
//! the daemon does. An `agent_browser_wait_for_text` call for the text `stub_hold` is never
//! answered (a call in flight; a core tool, so the guard's allowlist passes it); a request whose
//! id is a string starting `stub-error` gets a JSON-RPC error instead of a result. Two calls get
//! agent-browser's own answers ([`chrome_part::stand_in`]): an `open` of a `.pdf` URL, and the
//! profiles. Cargo builds it for the tests; it is never shipped.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "the test stub plays agent-browser's daemon: its socket, its pid file, its start"
)]
mod daemon {
    use std::os::unix::net::UnixListener;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    fn files() -> (PathBuf, String) {
        let dir = PathBuf::from(std::env::var_os("AGENT_BROWSER_SOCKET_DIR").unwrap_or_default());
        let session = std::env::var("AGENT_BROWSER_SESSION").unwrap_or_default();
        (dir, session)
    }

    /// The daemon: listens, records its pid, accepts and drops connections until killed.
    pub(crate) fn serve() -> std::io::Result<()> {
        let (dir, session) = files();
        let socket = dir.join(format!("{session}.sock"));
        drop(std::fs::remove_file(&socket));
        let listener = UnixListener::bind(socket)?;
        std::fs::write(
            dir.join(format!("{session}.pid")),
            std::process::id().to_string(),
        )?;
        for stream in listener.incoming() {
            drop(stream);
        }
        Ok(())
    }

    /// `--late`: records its pid, waits for `late-go`, then serves.
    pub(crate) fn late() -> std::io::Result<()> {
        let (dir, _) = files();
        std::fs::write(dir.join("late.pid"), std::process::id().to_string())?;
        while !dir.join("late-go").exists() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        serve()
    }

    /// Starts `daemon_stub <flag>` in a process group of its own, not waiting for it.
    pub(crate) fn start_alone(flag: &str) -> std::io::Result<()> {
        let child = Command::new(std::env::current_exe()?)
            .arg(flag)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        drop(child);
        Ok(())
    }

    /// `--linger`: records its pid and waits to be killed.
    pub(crate) fn linger() -> std::io::Result<()> {
        let (dir, _) = files();
        std::fs::write(dir.join("linger.pid"), std::process::id().to_string())?;
        wait_forever()
    }

    /// Blocks until the process is killed.
    fn wait_forever() -> ! {
        loop {
            std::thread::park();
        }
    }

    /// Starts `daemon_stub --linger` in this process's group, not waiting for it.
    pub(crate) fn start_linger() -> std::io::Result<()> {
        let child = Command::new(std::env::current_exe()?)
            .arg("--linger")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        drop(child);
        Ok(())
    }

    /// Starts the daemon in a group of its own and waits until its pid file exists.
    pub(crate) fn start() -> std::io::Result<()> {
        let (dir, session) = files();
        let pid_file = dir.join(format!("{session}.pid"));
        // A restarted session's previous daemon left its pid file: wait for this daemon's own.
        drop(std::fs::remove_file(&pid_file));
        let mut daemon = Command::new(std::env::current_exe()?)
            .arg("--daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        while !pid_file.exists() {
            if daemon.try_wait()?.is_some() {
                return Err(std::io::Error::other("the daemon stub exited"));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let guard = std::os::unix::process::parent_id();
        std::fs::write(
            dir.join(format!("daemon-of-{guard}.pid")),
            daemon.id().to_string(),
        )?;
        // Not reaped here: it outlives this process, as agent-browser's daemon does.
        drop(daemon);
        Ok(())
    }
}

/// What agent-browser's first tool call starts on Unix: the daemon, and with `linger` or
/// `late` the process the test asked for.
#[cfg(unix)]
fn first_call() -> std::io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("linger") {
        daemon::start_linger()?;
    }
    if std::env::args().nth(1).as_deref() == Some("late") {
        daemon::start_alone("--late")
    } else {
        daemon::start()
    }
}

fn main() -> std::io::Result<()> {
    #[cfg(unix)]
    match std::env::args().nth(1).as_deref() {
        Some("--daemon") => return daemon::serve(),
        Some("--linger") => return daemon::linger(),
        Some("--late") => return daemon::late(),
        _ => {}
    }
    let mut out = std::io::stdout().lock();
    let mut called = false;
    for line in std::io::stdin().lock().lines() {
        let Ok(message) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        if !called && message.get("method").and_then(Value::as_str) == Some("tools/call") {
            called = true;
            #[cfg(unix)]
            first_call()?;
        }
        // A call held unanswered, for the tests of a call in flight when Chrome dies.
        if message.pointer("/params/name").and_then(Value::as_str)
            == Some("agent_browser_wait_for_text")
            && message
                .pointer("/params/arguments/text")
                .and_then(Value::as_str)
                == Some("stub_hold")
        {
            continue;
        }
        if let Some(reply) = chrome_part::stand_in(&message) {
            writeln!(out, "{reply}")?;
            out.flush()?;
            continue;
        }
        if let Some(id) = message.get("id") {
            let reply = if id.as_str().is_some_and(|id| id.starts_with("stub-error")) {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": "stub"}})
            } else {
                json!({"jsonrpc": "2.0", "id": id, "result": {}})
            };
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Chrome's part in two calls, played with the files Chrome would write.
#[expect(
    clippy::disallowed_methods,
    reason = "the test stub plays Chrome: it writes, marks and renames a download in staging"
)]
mod chrome_part {
    use serde_json::{Value, json};

    /// agent-browser v0.41.1's core profile description (`cli/src/mcp.rs:257`).
    const CORE_PROFILE: &str = "Everyday browser automation with navigation, snapshots, \
        common interaction, waits, screenshots, basic reads, tab basics, JavaScript eval, close, \
        and profile discovery.";

    /// agent-browser's answer, with Chrome's part, to two calls. An `agent_browser_open` of a URL
    /// ending in `.pdf` plays Chrome saving the file (`plugins.always_open_pdf_externally`):
    /// `<name>.crdownload` appears in the run's staging folder at once, with the OS mark Chrome
    /// gives it, and is renamed to `<name>` 300 ms later, while the call fails with
    /// `net::ERR_ABORTED`, as the navigation turned into a download. An
    /// `agent_browser_tools_profiles` call gets the core profile's description.
    pub(super) fn stand_in(message: &Value) -> Option<Value> {
        let id = message.get("id")?;
        let text = match message.pointer("/params/name").and_then(Value::as_str)? {
            "agent_browser_open" => {
                let url = message.pointer("/params/arguments/url")?.as_str()?;
                let name = url.rsplit('/').next().filter(|name| {
                    std::path::Path::new(name)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
                })?;
                let staging = staging()?;
                let partial = staging.join(format!("{name}.crdownload"));
                std::fs::write(&partial, b"%PDF-1.4 stub judgment").ok()?;
                mark(&partial);
                let done = staging.join(name);
                drop(std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    drop(std::fs::rename(&partial, &done));
                }));
                let text = format!("Navigation failed: net::ERR_ABORTED at {url}");
                return Some(json!({"jsonrpc": "2.0", "id": id,
                    "result": {"content": [{"type": "text", "text": text}], "isError": true}}));
            }
            "agent_browser_tools_profiles" => json!([{"name": "core", "description": CORE_PROFILE,
                "toolCount": 29, "usage": "agent-browser mcp --tools core"}])
            .to_string(),
            _ => return None,
        };
        Some(json!({"jsonrpc": "2.0", "id": id,
            "result": {"content": [{"type": "text", "text": text}]}}))
    }

    /// The run's staging folder: `<cwd>/chrome-profile-<id>/downloads-staging`, the guard having
    /// started this stub in its `--cwd`.
    fn staging() -> Option<std::path::PathBuf> {
        std::fs::read_dir(".").ok()?.flatten().find_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let staging = entry.path().join("downloads-staging");
            (name.starts_with("chrome-profile-") && staging.is_dir()).then_some(staging)
        })
    }

    /// Chrome's OS mark on `file`: Windows' `Zone.Identifier` stream, macOS' quarantine attribute.
    fn mark(file: &std::path::Path) {
        if cfg!(windows) {
            let stream = format!("{}:Zone.Identifier", file.display());
            drop(std::fs::write(stream, "[ZoneTransfer]\r\nZoneId=3\r\n"));
        } else if cfg!(target_os = "macos") {
            let value = "0081;00000000;Chrome;";
            drop(
                std::process::Command::new("/usr/bin/xattr")
                    .args(["-w", "com.apple.quarantine", value])
                    .arg(file)
                    .status(),
            );
        }
    }
}
