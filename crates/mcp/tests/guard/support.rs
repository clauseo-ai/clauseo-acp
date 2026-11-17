//! Scratch folders, the stub children, and a running guard over pipes.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{
    Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Output, Stdio,
};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::write;

/// The guard binary under test.
pub(crate) const GUARD: &str = env!("CARGO_BIN_EXE_clauseo-mcp");

/// The proxy the tests give the guard.
pub(crate) const PROXY: &str = "http://127.0.0.1:9";
/// The proxy's password the tests' host gives every guard: obviously fake, 64 of `0-9 a-f`.
pub(crate) const PROXY_SECRET: &str =
    "0f1e2d3c4b5a69780f1e2d3c4b5a69780f1e2d3c4b5a69780f1e2d3c4b5a6978";
/// How many password files [`host_env`] has written in this process, so each has a name of
/// its own: the guard deletes its file at its start, and two guards may start at once.
static SECRET_FILES: AtomicU64 = AtomicU64::new(0);
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
        write::create_dir_all(path.join("program-files")).unwrap();
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

/// The variables the host sets, valid: the socket folder on Unix only. Like the host, it writes
/// a fresh password file for each guard (`CLAUSEO_PROXY_SECRET_FILE`, in the scratch folder),
/// which the guard deletes at its start: call it once per guard started.
pub(crate) fn host_env(scratch: &Scratch) -> Vec<(String, String)> {
    let path = |name: &str| scratch.path().join(name).to_str().unwrap().to_owned();
    let secret = format!(
        "proxy-secret-{}",
        SECRET_FILES.fetch_add(1, Ordering::Relaxed)
    );
    write::write(scratch.path().join(&secret), format!("{PROXY_SECRET}\n")).unwrap();
    let mut vars = vec![
        (
            "AGENT_BROWSER_CONFIG".to_owned(),
            path("agent-browser.json"),
        ),
        ("AGENT_BROWSER_PROXY".to_owned(), PROXY.to_owned()),
        ("CLAUSEO_PROXY_SECRET_FILE".to_owned(), path(&secret)),
        ("AGENT_BROWSER_EXECUTABLE_PATH".to_owned(), path(CHROME)),
        (
            "AGENT_BROWSER_SESSION".to_owned(),
            scratch.session().to_owned(),
        ),
    ];
    if cfg!(unix) {
        vars.push((
            "AGENT_BROWSER_SOCKET_DIR".to_owned(),
            scratch.socket_dir().to_str().unwrap().to_owned(),
        ));
    } else {
        // The guard's Program Files folders, an empty one in the scratch folder, so it never
        // starts the runner's real Edge or Chrome after the stand-in (a `test-support` knob).
        vars.push((
            "CLAUSEO_TEST_GUARD_PROGRAM_FILES".to_owned(),
            path("program-files"),
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

/// The processes still running (zombies not counted) whose command line contains the test's
/// scratch folder: the guard, its reaper, agent-browser and Chrome with every helper, whatever
/// process group each is in. Each as `<pid> <command line>`.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn left_in(scratch: &Path) -> Vec<String> {
    let needle = format!("{}/", scratch.display());
    process_table()
        .into_iter()
        .filter(|process| !process.zombie && process.mentions(&needle))
        .map(|process| format!("{} {}", process.pid, process.line))
        .collect()
}

/// One process, as the Unix tests read the process table ([`process_table`]).
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug)]
pub(crate) struct Process {
    pub(crate) pid: i32,
    /// Its process group.
    pub(crate) group: i32,
    pub(crate) zombie: bool,
    /// Its arguments, each whole (Linux: `/proc/<pid>/cmdline` split at its NULs); none on
    /// macOS, where `ps` joins them.
    args: Option<Vec<Vec<u8>>>,
    /// Its arguments joined by spaces.
    pub(crate) line: String,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Process {
    /// Whether one of its arguments starts with `prefix`. From `ps`'s line (macOS), an
    /// argument starts at the line's start or after a space.
    pub(crate) fn has_arg_starting(&self, prefix: &str) -> bool {
        match &self.args {
            Some(args) => args.iter().any(|arg| arg.starts_with(prefix.as_bytes())),
            None => format!(" {}", self.line).contains(&format!(" {prefix}")),
        }
    }

    /// Whether one of its arguments is `arg`. From `ps`'s line (macOS), an argument is
    /// what lies between spaces or the line's ends.
    pub(crate) fn has_arg(&self, arg: &str) -> bool {
        match &self.args {
            Some(args) => args.iter().any(|each| each == arg.as_bytes()),
            None => format!(" {} ", self.line).contains(&format!(" {arg} ")),
        }
    }

    /// Whether one of its arguments contains `needle` (from `ps`'s line on macOS, the line).
    pub(crate) fn mentions(&self, needle: &str) -> bool {
        match &self.args {
            Some(args) => args.iter().any(|arg| {
                arg.windows(needle.len())
                    .any(|window| window == needle.as_bytes())
            }),
            None => self.line.contains(needle),
        }
    }
}

/// Linux: every process in `/proc` whose `stat` is readable, zombies too.
#[cfg(target_os = "linux")]
pub(crate) fn process_table() -> Vec<Process> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let (_, rest) = stat.rsplit_once(") ")?;
            let fields: Vec<&str> = rest.split(' ').collect();
            let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            Some(Process {
                pid,
                group: fields.get(2)?.parse().ok()?,
                zombie: fields.first() == Some(&"Z"),
                line: String::from_utf8_lossy(&cmdline).replace('\0', " "),
                args: Some(cmdline.split(|b| *b == 0).map(<[u8]>::to_vec).collect()),
            })
        })
        .collect()
}

/// macOS: every process `ps -ax` lists, zombies too, with its whole command line.
#[cfg(target_os = "macos")]
pub(crate) fn process_table() -> Vec<Process> {
    let mut ps = Command::new("/bin/ps");
    ps.args(["-axww", "-o", "pid=,pgid=,stat=,command="]);
    let output = run(ps);
    assert!(
        output.status.success(),
        "ps failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = [""; 3];
            let mut rest = line.trim_start();
            for field in &mut fields {
                let (value, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                *field = value;
                rest = tail.trim_start();
            }
            let [pid, group, stat] = fields;
            Some(Process {
                pid: pid.parse().ok()?,
                group: group.parse().ok()?,
                zombie: stat.starts_with('Z'),
                args: None,
                line: rest.to_owned(),
            })
        })
        .collect()
}

/// A tool call with `id`: `agent_browser_open` on a public address, which the filter passes.
pub(crate) fn open_call(id: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": "agent_browser_open", "arguments": {"url": "https://example.com"}}})
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

    /// Sends a tool call with `id`, which starts the browser if it is stopped (the guard's lazy
    /// start), without reading its answer.
    pub(crate) fn send_call(&mut self, id: &str) {
        self.send(&open_call(id));
    }

    /// Sends a tool call and returns its answer: once it is back, Chrome's stand-in and
    /// agent-browser's run.
    pub(crate) fn call(&mut self, id: &str) -> Value {
        self.send_call(id);
        let answer = self.read();
        assert_eq!(answer["id"], id, "{answer}");
        answer
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

    /// Writes the messages `make` gives (the n-th for `n`) as fast as the guard takes them,
    /// throwing its stdout away meanwhile, until a write fails or the guard has exited; then
    /// closes stdin and returns the exit status.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn flood(&mut self, mut make: impl FnMut(u64) -> Value) -> ExitStatus {
        let Self {
            child,
            stdin,
            stdout,
            ..
        } = self;
        let mut stdin = stdin.take().expect("stdin is closed");
        std::thread::scope(|scope| {
            scope.spawn(|| drop(std::io::copy(stdout, &mut std::io::sink())));
            for n in 0.. {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                let line = format!("{}\n", make(n));
                if stdin.write_all(line.as_bytes()).is_err() {
                    break;
                }
            }
            drop(stdin);
            child.wait().unwrap()
        })
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

    /// [`Self::finish`], with stderr too: the exit status, then what stdout still held and
    /// stderr's text.
    pub(crate) fn finish_with_stderr(mut self) -> (ExitStatus, (String, String)) {
        self.close_stdin();
        let rest = self.read_rest();
        let status = self.wait();
        (status, (rest, self.stderr_text()))
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

    /// What the guard's stderr holds read but unused, and a second handle to it, for a failure's
    /// message to read the rest from on a thread of its own: a process the guard started may hold
    /// stderr open past the guard's exit.
    #[cfg(unix)]
    pub(crate) fn stderr_handle(&mut self) -> (String, Option<std::fs::File>) {
        use std::os::fd::AsFd;
        let read = String::from_utf8_lossy(self.stderr.buffer()).into_owned();
        let fd = self.stderr.get_ref().as_fd().try_clone_to_owned().ok();
        (read, fd.map(std::fs::File::from))
    }

    /// Reads stderr to its end; call once stdout has ended.
    pub(crate) fn stderr_text(&mut self) -> String {
        let mut text = String::new();
        drop(self.stderr.read_to_string(&mut text));
        text
    }
}

impl Drop for Guard {
    /// Kills the guard, unless a failed test on Windows drops it: there a kill skips the guard's
    /// own stop, and the processes in its job (`src/browser_guard/job.rs`) end only after its
    /// handle closes, still holding files in the test's folder, whose removal then fails
    /// (`os error 5`, `os error 32`) and hides the failure's own message. There the guard's stdin
    /// closes instead, as at the end of a test, and the drop waits for the guard to end itself,
    /// with everything it started.
    fn drop(&mut self) {
        if cfg!(windows) && std::thread::panicking() {
            self.close_stdin();
            drop(std::io::copy(&mut self.stdout, &mut std::io::sink()));
        } else {
            drop(self.child.kill());
        }
        drop(self.child.wait());
    }
}
