//! Scratch folders, the stub children, and a running guard over pipes.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{
    Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Output, Stdio,
};

use serde_json::Value;

use crate::write;

/// The guard binary under test.
pub(crate) const GUARD: &str = env!("CARGO_BIN_EXE_clauseo-mcp");

/// The proxy the tests give the guard.
pub(crate) const PROXY: &str = "http://127.0.0.1:9";
/// The stream port the tests give the guard.
pub(crate) const STREAM_PORT: &str = "61100";
/// The Chrome stand-in's name in the scratch folder.
pub(crate) const CHROME: &str = if cfg!(windows) {
    "chrome.exe"
} else {
    "chrome"
};
/// The CDP port the tests give the guard.
pub(crate) const CDP_PORT: &str = "61101";
/// The daemon port the tests give the guard on Windows.
pub(crate) const DAEMON_PORT: &str = "61102";

/// A folder under the system temp folder, unique per test and process, removed on drop, with
/// the host's config file and a copy of the `chrome_stub` example (the Chrome the guard starts)
/// in it. Each also has its own session id, whose
/// first 16 characters name the Unix socket folder `/tmp/clauseo-ab-<id>`, which the test makes
/// as the host does (mode 0700) and removes on drop too.
pub(crate) struct Scratch {
    path: PathBuf,
    session: String,
}

impl Scratch {
    pub(crate) fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!("clauseo-mcp-{test}-{}", std::process::id()));
        if path.exists() {
            write::remove_dir_all(&path).unwrap();
        }
        write::create_dir_all(path.join("state")).unwrap();
        write::write(path.join("agent-browser.json"), "{}").unwrap();
        write::copy(example("chrome_stub"), path.join(CHROME)).unwrap();
        // FNV-1a of the test's name, then the process id: 16 hex characters, unique per test.
        let hash = test.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        });
        let session = format!("{hash:08x}{:08x}-t", std::process::id());
        let scratch = Self { path, session };
        #[cfg(unix)]
        {
            let socket_dir = scratch.socket_dir();
            if std::fs::symlink_metadata(&socket_dir).is_ok() {
                write::remove_dir_all(&socket_dir).unwrap();
            }
            write::create_dir_all(&socket_dir).unwrap();
            write::set_mode(&socket_dir, 0o700).unwrap();
        }
        scratch
    }

    /// The session id this test gives the guard.
    pub(crate) fn session(&self) -> &str {
        &self.session
    }

    /// The socket folder the host gives the guard on Unix.
    pub(crate) fn socket_dir(&self) -> PathBuf {
        PathBuf::from(format!("/tmp/clauseo-ab-{}", &self.session[..16]))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The folder given as `--cwd`.
    pub(crate) fn cwd(&self) -> PathBuf {
        self.path.join("state")
    }

    /// What the Chrome stand-in does (`examples/chrome_stub.rs`).
    pub(crate) fn chrome_mode(&self, mode: &str) {
        write::write(self.path.join("chrome-mode"), mode).unwrap();
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
        let socket_dir = self.socket_dir();
        if cfg!(unix)
            && std::fs::symlink_metadata(&socket_dir).is_ok()
            && let Err(error) = write::remove_dir_all(&socket_dir)
        {
            eprintln!("could not remove {}: {error}", socket_dir.display());
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

/// The variables the host sets, valid: the socket folder on Unix only.
pub(crate) fn host_env(scratch: &Scratch) -> Vec<(String, String)> {
    let path = |name: &str| scratch.path().join(name).to_str().unwrap().to_owned();
    let mut vars = vec![
        (
            "AGENT_BROWSER_CONFIG".to_owned(),
            path("agent-browser.json"),
        ),
        ("AGENT_BROWSER_PROXY".to_owned(), PROXY.to_owned()),
        ("AGENT_BROWSER_EXECUTABLE_PATH".to_owned(), path(CHROME)),
        (
            "AGENT_BROWSER_SESSION".to_owned(),
            scratch.session().to_owned(),
        ),
        (
            "AGENT_BROWSER_STREAM_PORT".to_owned(),
            STREAM_PORT.to_owned(),
        ),
    ];
    if cfg!(unix) {
        vars.push((
            "AGENT_BROWSER_SOCKET_DIR".to_owned(),
            scratch.socket_dir().to_str().unwrap().to_owned(),
        ));
    }
    vars
}

/// `--cdp-port <port>`, and `--daemon-port <port>` on Windows: the ports the host passes.
pub(crate) fn port_args() -> Vec<&'static str> {
    let mut args = vec!["--cdp-port", CDP_PORT];
    if cfg!(windows) {
        args.extend(["--daemon-port", DAEMON_PORT]);
    }
    args
}

/// `clauseo-mcp browser-guard --agent-browser <stub> --cwd <cwd> --cdp-port <port> -- <child_args>`
/// (with `--daemon-port` on Windows), with the host's variables on top of the test's own
/// environment.
pub(crate) fn guard_command(scratch: &Scratch, stub: &Path, child_args: &[&OsStr]) -> Command {
    let mut command = Command::new(GUARD);
    command
        .args(guard_args(scratch, stub, child_args))
        .envs(host_env(scratch));
    command
}

/// [`guard_command`]'s arguments.
pub(crate) fn guard_args(scratch: &Scratch, stub: &Path, child_args: &[&OsStr]) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["browser-guard".into(), "--agent-browser".into()];
    args.push(stub.into());
    args.push("--cwd".into());
    args.push(scratch.cwd().into());
    args.extend(port_args().into_iter().map(OsString::from));
    args.push("--".into());
    args.extend(child_args.iter().map(OsString::from));
    args
}

/// The runs' profile folders and lock files under `cwd`: `chrome-profile-<run id>` and
/// `chrome-profile-<run id>.lock` (`src/browser_guard/runs.rs`), sorted.
pub(crate) fn run_files(cwd: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(cwd)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with("chrome-profile-"))
                })
                .map(|entry| entry.path())
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

/// The runs' profile folders under `cwd`.
pub(crate) fn profiles(cwd: &Path) -> Vec<PathBuf> {
    run_files(cwd)
        .into_iter()
        .filter(|path| path.is_dir())
        .collect()
}

/// The pid file of the run's agent-browser daemon in `socket_dir`, `run-<run id>.pid`, if there
/// is one (the Unix session name is the run's own, `src/browser_guard/runs.rs`).
#[cfg(unix)]
pub(crate) fn daemon_pid_file(socket_dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(socket_dir)
        .ok()?
        .flatten()
        .find_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            name.strip_prefix("run-")?.strip_suffix(".pid")?;
            Some(entry.path())
        })
}

/// Linux: the processes still running (zombies not counted) whose command line contains the
/// test's scratch folder: the guard, its reaper, agent-browser and Chrome with every helper,
/// whatever process group each is in. Each as `<pid> <command line>`.
#[cfg(target_os = "linux")]
pub(crate) fn left_in(scratch: &Path) -> Vec<String> {
    let mut needle = scratch.as_os_str().as_encoded_bytes().to_vec();
    needle.push(b'/');
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let (_, rest) = stat.rsplit_once(") ")?;
            if rest.starts_with('Z') {
                return None;
            }
            let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
            cmdline
                .windows(needle.len())
                .any(|window| window == needle.as_slice())
                .then(|| {
                    format!(
                        "{pid} {}",
                        String::from_utf8_lossy(&cmdline).replace('\0', " ")
                    )
                })
        })
        .collect()
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
    stderr: BufReader<ChildStderr>,
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
        let stderr = BufReader::new(child.stderr.take().unwrap());
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

    /// The process id, for the Unix tests that signal it.
    #[cfg(unix)]
    pub(crate) fn pid(&self) -> u32 {
        self.child.id()
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

    /// Reads stderr lines until one contains `needle`, and returns that line. Panics at EOF.
    #[cfg(unix)]
    pub(crate) fn stderr_until(&mut self, needle: &str) -> String {
        loop {
            let mut line = String::new();
            let read = self.stderr.read_line(&mut line).unwrap();
            assert!(read > 0, "the guard closed stderr before {needle:?}");
            if line.contains(needle) {
                return line;
            }
        }
    }

    /// Reads stderr to its end; call once stdout has ended.
    pub(crate) fn stderr_text(&mut self) -> String {
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
