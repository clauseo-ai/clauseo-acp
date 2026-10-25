//! Drives the built `fake_agent` over real pipes. No sleeps, timeouts or polling: every wait is a
//! blocking read of a line the agent writes, or of EOF.
//!
//! So the only guard against a hang is nextest's slow-timeout in `.config/nextest.toml` (period
//! 10 s, terminate-after 3: a test still running after 30 s is killed). Run these tests with
//! `cargo nextest run`: under plain `cargo test` a read the agent never answers blocks forever.
//!
//! Reuse it for a new scenario test:
//! ```ignore
//! let dir = Scratch::new("my_test");
//! let mut agent = Agent::start("prompt-stream", dir.path());
//! agent.handshake(dir.path());
//! agent.send(json!({"jsonrpc": "2.0", "id": 2, "method": "session/prompt", "params": {...}}));
//! assert_eq!(agent.read_line()["params"]["update"]["sessionUpdate"], "agent_thought_chunk");
//! agent.expect_exit(EXIT_OK);
//! ```

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};

use serde_json::{Value, json};

use crate::write;

/// A folder under the system temp folder, unique per test and process, removed on drop.
pub(crate) struct Scratch {
    path: PathBuf,
}

impl Scratch {
    pub(crate) fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!("fake_agent-{test}-{}", std::process::id()));
        if path.exists() {
            write::remove_dir_all(&path).unwrap();
        }
        write::create_dir_all(&path).unwrap();
        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The path as the JSON string a client sends as `cwd`.
    pub(crate) fn cwd(&self) -> String {
        self.path.to_str().unwrap().to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = write::remove_dir_all(&self.path) {
            // A test that failed may leave a file open on Windows; say so rather than panic.
            eprintln!("could not remove {}: {error}", self.path.display());
        }
    }
}

/// A running `fake_agent` with piped stdin, stdout and stderr. Killed on drop if still running.
pub(crate) struct Agent {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    /// `None` once [`Agent::finish`] has handed it to its reader thread.
    stderr: Option<BufReader<ChildStderr>>,
}

/// What an agent left behind when it exited.
pub(crate) struct Finished {
    pub(crate) status: ExitStatus,
    /// Every stdout line after the last one the test read.
    pub(crate) stdout: String,
    /// Every stderr line after the last one the test read.
    pub(crate) stderr: String,
}

impl Agent {
    /// Starts `fake_agent --scenario <scenario>` with `dir` as its working directory.
    pub(crate) fn start(scenario: &str, dir: &Path) -> Self {
        Self::start_args(&["--scenario", scenario], dir)
    }

    /// Starts `fake_agent` with any arguments.
    #[expect(
        clippy::disallowed_methods,
        reason = "the test starts the binary it tests over pipes"
    )]
    pub(crate) fn start_args(args: &[&str], dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fake_agent"))
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let stderr = Some(BufReader::new(child.stderr.take().unwrap()));
        Self {
            child,
            stdin,
            stdout,
            stderr,
        }
    }

    pub(crate) fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Writes `value` as one compact JSON line.
    pub(crate) fn send(&mut self, value: &Value) {
        self.send_raw(&value.to_string());
    }

    /// Writes `line` and `\n` verbatim.
    pub(crate) fn send_raw(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("stdin is closed");
        stdin.write_all(line.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    /// Writes `text` with no `\n` added.
    pub(crate) fn send_unterminated(&mut self, text: &str) {
        let stdin = self.stdin.as_mut().expect("stdin is closed");
        stdin.write_all(text.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }

    /// Closes the agent's stdin.
    pub(crate) fn close_stdin(&mut self) {
        self.stdin = None;
    }

    /// The next stdout line, without its `\n`. Panics at EOF with the agent's stderr.
    pub(crate) fn read_raw(&mut self) -> String {
        let mut line = String::new();
        if self.stdout.read_line(&mut line).unwrap() == 0 {
            let mut stderr = String::new();
            self.stderr_reader().read_to_string(&mut stderr).unwrap();
            panic!("stdout closed; stderr:\n{stderr}");
        }
        assert!(line.ends_with('\n'), "line without \\n: {line}");
        line.pop();
        line
    }

    /// The next stdout line, parsed as JSON.
    pub(crate) fn read_line(&mut self) -> Value {
        let line = self.read_raw();
        serde_json::from_str(&line).unwrap_or_else(|error| panic!("{error}: {line}"))
    }

    /// Asserts the next stdout line equals `expected`.
    pub(crate) fn expect_line(&mut self, expected: &Value) {
        assert_eq!(&self.read_line(), expected);
    }

    /// The next stderr line, without its `\n`. Panics at EOF.
    pub(crate) fn stderr_line(&mut self) -> String {
        let mut line = String::new();
        assert_ne!(
            self.stderr_reader().read_line(&mut line).unwrap(),
            0,
            "stderr closed"
        );
        line.pop();
        line
    }

    fn stderr_reader(&mut self) -> &mut BufReader<ChildStderr> {
        self.stderr.as_mut().unwrap()
    }

    /// Closes stdin, reads stdout and stderr to EOF, and waits for the exit.
    ///
    /// Stderr is read on its own thread while this one reads stdout. A Windows pipe holds only
    /// 4 KiB, so reading one after the other would deadlock on an agent that writes more stderr
    /// (exit-mid-turn writes 8,000 bytes) before it closes stdout.
    pub(crate) fn finish(mut self) -> Finished {
        self.close_stdin();
        let mut reader = self.stderr.take().unwrap();
        let stderr_thread = std::thread::spawn(move || {
            let mut stderr = String::new();
            reader.read_to_string(&mut stderr).unwrap();
            stderr
        });
        let mut stdout = String::new();
        self.stdout.read_to_string(&mut stdout).unwrap();
        let stderr = stderr_thread.join().unwrap();
        let status = self.child.wait().unwrap();
        Finished {
            status,
            stdout,
            stderr,
        }
    }

    /// Like [`Agent::finish`], and asserts the exit code and that stdout has nothing more.
    pub(crate) fn expect_exit(self, code: i32) -> Finished {
        let finished = self.finish();
        assert_eq!(
            finished.status.code(),
            Some(code),
            "stderr:\n{}",
            finished.stderr
        );
        assert_eq!(finished.stdout, "", "unread stdout");
        finished
    }

    /// Kills the agent this test started (by its PID, through `Child::kill`).
    pub(crate) fn kill(&mut self) {
        self.child.kill().unwrap();
    }

    /// Whether the agent is still running, without waiting.
    pub(crate) fn running(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }

    /// Plays the client side of `scenarios/common/handshake.jsonl` with `cwd`: `initialize` as
    /// id 0 and `session/new` as id 1, asserting both responses.
    pub(crate) fn handshake(&mut self, cwd: &str) {
        self.send(&request(
            0,
            "initialize",
            json!({"protocolVersion": 1, "clientCapabilities": {}}),
        ));
        self.expect_line(&json!({"jsonrpc": "2.0", "id": 0, "result": {
            "protocolVersion": 1, "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": env!("CARGO_PKG_VERSION")}}}));
        self.send(&request(
            1,
            "session/new",
            json!({"cwd": cwd, "mcpServers": []}),
        ));
        self.expect_line(&json!({"jsonrpc": "2.0", "id": 1, "result": {"sessionId": "sess-1"}}));
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) && self.child.kill().is_ok() {
            let _status = self.child.wait();
        }
    }
}

/// A JSON-RPC request from the client.
pub(crate) fn request(id: u64, method: &str, params: Value) -> Value {
    let mut message = json!({"jsonrpc": "2.0", "id": id, "method": method});
    message["params"] = params;
    message
}

/// A JSON-RPC notification from the client.
pub(crate) fn notification(method: &str, params: Value) -> Value {
    let mut message = json!({"jsonrpc": "2.0", "method": method});
    message["params"] = params;
    message
}

/// A `session/prompt` request for `sess-1` with one text block.
pub(crate) fn prompt(id: u64, text: &str) -> Value {
    request(
        id,
        "session/prompt",
        json!({"sessionId": "sess-1", "prompt": [{"type": "text", "text": text}]}),
    )
}

/// A `session/update` notification the agent writes for `sess-1`.
pub(crate) fn update(update: Value) -> Value {
    let mut message =
        json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "sess-1"}});
    message["params"]["update"] = update;
    message
}

/// The agent's response to the client's request `id`.
pub(crate) fn result(id: u64, result: Value) -> Value {
    let mut message = json!({"jsonrpc": "2.0", "id": id});
    message["result"] = result;
    message
}

/// Kills a process the test started and read the PID of: `kill -9` on Unix, `taskkill /F` on
/// Windows. Never by name. Returns whether the kill command succeeded.
#[expect(
    clippy::disallowed_methods,
    reason = "kills the grandchild PID this test read"
)]
pub(crate) fn kill_pid(pid: u32) -> bool {
    let pid = pid.to_string();
    let mut command = if cfg!(windows) {
        let mut command = Command::new("taskkill");
        command.args(["/F", "/PID", &pid]);
        command
    } else {
        let mut command = Command::new("kill");
        command.args(["-9", &pid]);
        command
    };
    let status = command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .and_then(|mut child| child.wait());
    status.is_ok_and(|status| status.success())
}

/// Kills a PID the test read (the grandchild) when dropped, so a failed assert cannot leave a
/// `--hold` process running.
pub(crate) struct PidGuard {
    pid: Option<u32>,
}

impl PidGuard {
    pub(crate) fn new(pid: u32) -> Self {
        Self { pid: Some(pid) }
    }

    /// Kills the process now and asserts the kill succeeded.
    pub(crate) fn kill(mut self) {
        let pid = self.pid.take().unwrap();
        assert!(kill_pid(pid), "could not kill {pid}");
    }
}

impl Drop for PidGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.pid.take() {
            // Already unwinding from a failed assert: never panic here.
            let _killed = kill_pid(pid);
        }
    }
}
