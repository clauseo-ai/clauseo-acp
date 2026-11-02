//! Scratch folders, the stub children, and a running guard over pipes.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{
    Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Output, Stdio,
};

use serde_json::Value;

use crate::write;

/// The guard binary under test.
pub(crate) const GUARD: &str = env!("CARGO_BIN_EXE_clauseo-mcp");

/// The session id the tests give the guard.
pub(crate) const SESSION: &str = "thread-1_A";
/// The proxy the tests give the guard.
pub(crate) const PROXY: &str = "http://127.0.0.1:9";

/// A folder under the system temp folder, unique per test and process, removed on drop.
pub(crate) struct Scratch {
    path: PathBuf,
}

impl Scratch {
    pub(crate) fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!("clauseo-mcp-{test}-{}", std::process::id()));
        if path.exists() {
            write::remove_dir_all(&path).unwrap();
        }
        write::create_dir_all(path.join("state")).unwrap();
        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The folder given as `--cwd`.
    pub(crate) fn cwd(&self) -> PathBuf {
        self.path.join("state")
    }

    /// The echo stub's log.
    pub(crate) fn log(&self) -> PathBuf {
        self.path.join("received.jsonl")
    }

    /// Every line the echo stub received, parsed.
    pub(crate) fn received(&self) -> Vec<Value> {
        let text = std::fs::read_to_string(self.log()).unwrap_or_default();
        text.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = write::remove_dir_all(&self.path) {
            eprintln!("could not remove {}: {error}", self.path.display());
        }
    }
}

/// A stub built from `examples/`. `cargo test` and `cargo nextest run` build the examples
/// beside the test binaries' `deps/` folder.
pub(crate) fn example(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().and_then(Path::parent).unwrap();
    let path = profile
        .join("examples")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "{} is missing; cargo test builds the examples",
        path.display()
    );
    path
}

/// The variables the host sets, valid.
pub(crate) fn host_env(scratch: &Scratch) -> Vec<(String, String)> {
    let path = |name: &str| scratch.path().join(name).to_str().unwrap().to_owned();
    vec![
        (
            "AGENT_BROWSER_CONFIG".to_owned(),
            path("agent-browser.json"),
        ),
        ("AGENT_BROWSER_PROXY".to_owned(), PROXY.to_owned()),
        ("AGENT_BROWSER_EXECUTABLE_PATH".to_owned(), path("chrome")),
        ("AGENT_BROWSER_SESSION".to_owned(), SESSION.to_owned()),
    ]
}

/// `clauseo-mcp browser-guard --agent-browser <stub> --cwd <cwd> -- <child_args>`, with the host's
/// variables on top of the test's own environment.
pub(crate) fn guard_command(scratch: &Scratch, stub: &Path, child_args: &[&OsStr]) -> Command {
    let mut command = Command::new(GUARD);
    command
        .arg("browser-guard")
        .arg("--agent-browser")
        .arg(stub)
        .arg("--cwd")
        .arg(scratch.cwd())
        .arg("--")
        .args(child_args)
        .envs(host_env(scratch));
    command
}

/// Runs a command to its end with stdin closed.
#[expect(
    clippy::disallowed_methods,
    reason = "the test runs the binary it tests"
)]
pub(crate) fn run(mut command: Command) -> Output {
    command.stdin(Stdio::null()).output().unwrap()
}

/// A running guard with piped stdin, stdout and stderr. Killed on drop if still running.
pub(crate) struct Guard {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    stderr: ChildStderr,
}

impl Guard {
    #[expect(
        clippy::disallowed_methods,
        reason = "the test starts the binary it tests over pipes"
    )]
    pub(crate) fn start(mut command: Command) -> Self {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let stderr = child.stderr.take().unwrap();
        Self {
            child,
            stdin,
            stdout,
            stderr,
        }
    }

    /// Writes `line` and a newline.
    pub(crate) fn send_raw(&mut self, line: &[u8]) {
        let stdin = self.stdin.as_mut().expect("stdin is closed");
        stdin.write_all(line).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    pub(crate) fn send(&mut self, message: &Value) {
        self.send_raw(message.to_string().as_bytes());
    }

    /// The next stdout line, parsed. Panics at EOF.
    pub(crate) fn read(&mut self) -> Value {
        let mut line = String::new();
        let read = self.stdout.read_line(&mut line).unwrap();
        assert!(
            read > 0,
            "the guard closed stdout; stderr: {}",
            self.stderr_text()
        );
        assert!(line.ends_with('\n'), "a whole line: {line:?}");
        serde_json::from_str(&line).unwrap()
    }

    /// Reads stdout to its end, leaving stdin open.
    pub(crate) fn read_rest(&mut self) -> String {
        let mut rest = String::new();
        self.stdout.read_to_string(&mut rest).unwrap();
        rest
    }

    pub(crate) fn wait(&mut self) -> ExitStatus {
        self.child.wait().unwrap()
    }

    pub(crate) fn close_stdin(&mut self) {
        self.stdin = None;
    }

    /// Closes stdin, then returns the exit status and whatever stdout still held.
    pub(crate) fn finish(mut self) -> (ExitStatus, String) {
        self.close_stdin();
        let mut rest = String::new();
        self.stdout.read_to_string(&mut rest).unwrap();
        (self.child.wait().unwrap(), rest)
    }

    fn stderr_text(&mut self) -> String {
        let mut text = String::new();
        drop(self.stderr.read_to_string(&mut text));
        text
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        drop(self.child.kill());
        drop(self.child.wait());
    }
}
