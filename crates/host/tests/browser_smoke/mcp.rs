//! The guard started as the host's browser entry starts it, and MCP JSON-RPC over its stdio.

#![expect(
    clippy::disallowed_methods,
    reason = "a by-hand smoke test: it starts the guard, writes its own folder and waits on the \
              wall clock"
)]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::scratch::Scratch;

/// What the guard passes on from its own environment (`browser_guard/env.rs`, `PASS_*`).
const PASSED: &[&str] = if cfg!(windows) {
    &[
        "SystemRoot",
        "windir",
        "SystemDrive",
        "ComSpec",
        "PATHEXT",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "LOCALAPPDATA",
        "APPDATA",
        "USERPROFILE",
    ]
} else {
    &["LANG"]
};

/// Calls `check` every 100 ms until it holds or `limit` passes; returns whether it held.
pub(crate) fn poll(limit: Duration, mut check: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if check() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// The guard as the host's browser entry starts it, with a cleared environment.
pub(crate) fn guard_command(
    mcp: &Path,
    agent_browser: &Path,
    chrome: &Path,
    scratch: &Scratch,
    proxy_port: u16,
) -> Command {
    let mut command = Command::new(mcp);
    command
        .arg("browser-guard")
        .arg("--agent-browser")
        .arg(agent_browser)
        .arg("--cwd")
        .arg(scratch.cwd())
        .args(["--", "mcp", "--tools", "core"])
        .env_clear()
        .env("AGENT_BROWSER_CONFIG", scratch.root.join("config.json"))
        .env(
            "AGENT_BROWSER_PROXY",
            format!("http://127.0.0.1:{proxy_port}"),
        )
        .env("AGENT_BROWSER_SESSION", &scratch.session)
        .env("AGENT_BROWSER_EXECUTABLE_PATH", chrome);
    for name in PASSED {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

/// A running guard: requests go to its stdin, its stdout lines arrive on a channel so
/// every read has a timeout. Dropping it kills a guard that is still running.
pub(crate) struct Guard {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
}

impl Guard {
    pub(crate) fn start(mut command: Command) -> Self {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start clauseo-mcp browser-guard");
        let stdout = child.stdout.take().expect("guard stdout");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        Self {
            child,
            stdin,
            lines,
            next_id: 0,
        }
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("guard stdin is open");
        writeln!(stdin, "{message}").expect("write to the guard");
        stdin.flush().expect("flush the guard's stdin");
    }

    pub(crate) fn notify(&mut self, method: &str) {
        self.send(&json!({"jsonrpc": "2.0", "method": method}));
    }

    /// Sends a request and returns its `result`, skipping other lines; fails after `limit`.
    pub(crate) fn request(&mut self, method: &str, params: &Value, limit: Duration) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let deadline = Instant::now() + limit;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self
                .lines
                .recv_timeout(left)
                .unwrap_or_else(|error| panic!("no reply to {method} within {limit:?}: {error}"));
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                eprintln!("guard stdout, not JSON: {line}");
                continue;
            };
            if message["id"] == json!(id) {
                assert!(message.get("error").is_none(), "{method} failed: {message}");
                return message["result"].clone();
            }
            eprintln!("guard stdout, skipped: {line}");
        }
    }

    /// `tools/call`; fails when the tool reports an error.
    pub(crate) fn call(&mut self, tool: &str, arguments: &Value, limit: Duration) -> Value {
        let params = json!({"name": tool, "arguments": arguments});
        let result = self.request("tools/call", &params, limit);
        assert_ne!(result["isError"], json!(true), "{tool} failed: {result}");
        result
    }

    /// Closes stdin and waits for the guard to exit; kills it and fails after `limit`.
    pub(crate) fn finish(mut self, limit: Duration) -> ExitStatus {
        drop(self.stdin.take());
        let mut status = None;
        let exited = poll(limit, || {
            status = self.child.try_wait().expect("wait for the guard");
            status.is_some()
        });
        assert!(
            exited,
            "the guard did not exit within {limit:?} of its stdin closing"
        );
        status.expect("exit status")
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            self.child.kill().unwrap_or_default();
            self.child.wait().map(drop).unwrap_or_default();
        }
    }
}
