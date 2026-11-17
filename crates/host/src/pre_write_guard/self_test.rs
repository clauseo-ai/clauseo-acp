//! The launch's self-test of Codex's guard hook: before a Codex session's first prompt, and after
//! its id went in the ids file, the host runs the hook's configured command line as Codex 0.162.1
//! runs it (under the session's shell, [`argv`]: PowerShell `-NoProfile -Command <line>` on
//! Windows, the account's login shell `-c <line>` elsewhere; the environment cleared, then the
//! agent's; the session folder as the working folder) twice:
//!
//! 1. with a call from a session no conversation has ([`input`]), which the guard must refuse:
//!    exit 2 with something on stderr, within [`LIMIT`];
//! 2. with a harmless call from the conversation's own session ([`own_input`]), which the guard
//!    must allow: exit 0, nothing or a JSON value on stdout, within [`LIMIT`].
//!
//! The launch goes on only when both hold; anything else (no guard at that path, one an antivirus
//! stops, one that hangs or crashes, one that answers every call the same way, an ids file it
//! can't read) refuses the launch (`agent.guard_selftest_failed`). So does exit 2 with
//! [`UNCHECKED`](super::UNCHECKED), the Windows line's own ending when the guard did not answer
//! (`agent/loads/codex.rs`, `ps_command`). Blocking: run off the actor
//! (`agent/codex/prepare.rs`).
//!
//! Its shape follows the shared guard self-test's (`run(entry, input, cwd, …)`, [`command_of`],
//! [`Failed`], [`LIMIT`]), so the two can become one.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

/// How long the hook may take to refuse, process start included: [`limit_for`] this OS.
pub(crate) const LIMIT: Duration = limit_for(cfg!(windows));

/// The self-test's limit on Windows when `windows`: 30 s there, the allowance Codex's hook
/// timeout gives a cold PowerShell start (`agent/loads/codex.rs`,
/// `CODEX_GUARD_TIMEOUT_SECONDS`), 5 s elsewhere, where the shell starts at once.
pub(crate) const fn limit_for(windows: bool) -> Duration {
    if windows {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(5)
    }
}

/// The most stderr read.
const MAX_OUTPUT: u64 = 64 * 1024;

/// The hook as Codex will run it for one session: Codex's `hooks` object, the agent's
/// environment and the session folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelfTest {
    pub(crate) entry: Map<String, Value>,
    pub(crate) env: BTreeMap<OsString, OsString>,
    pub(crate) cwd: PathBuf,
}

impl SelfTest {
    /// [`run`] on this hook with [`input`], then [`allows`] with [`own_input`] for the
    /// conversation's session `session`.
    pub(crate) fn run(&self, session: &str) -> Result<(), Failed> {
        run(Some(&self.entry), &input(&self.cwd), &self.cwd, &self.env)?;
        let own = own_input(&self.cwd, session);
        allows(Some(&self.entry), &own, &self.cwd, &self.env)
    }
}

/// Why the self-test failed, for the log (no path, no stderr text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failed {
    /// No hook command in the hooks object.
    NoEntry,
    /// The shell could not be started.
    Launch,
    /// It did not answer within the limit, and was killed.
    Late,
    /// It answered without refusing: another exit code, or nothing on stderr.
    Answer,
    /// It did not allow the conversation's own harmless call: another exit code than 0, or
    /// something on stdout that is not JSON.
    Refused,
}

/// The made-up hook input: a `Bash` call from a session no conversation has, in `cwd`.
pub(crate) fn input(cwd: &Path) -> Vec<u8> {
    serde_json::json!({
        "session_id": "clauseo-self-test",
        "turn_id": "clauseo-self-test",
        "transcript_path": null,
        "cwd": cwd.to_string_lossy(),
        "hook_event_name": "PreToolUse",
        "model": "",
        "permission_mode": "default",
        "tool_name": "Bash",
        "tool_input": {"command": "true"},
        "tool_use_id": "clauseo-self-test",
    })
    .to_string()
    .into_bytes()
}

/// A harmless call from the conversation's session `session`, in `cwd`: `update_plan` with an
/// empty plan, a tool the guard checks no path of, so the session check alone decides it.
pub(crate) fn own_input(cwd: &Path, session: &str) -> Vec<u8> {
    serde_json::json!({
        "session_id": session,
        "turn_id": "clauseo-self-test",
        "transcript_path": null,
        "cwd": cwd.to_string_lossy(),
        "hook_event_name": "PreToolUse",
        "model": "",
        "permission_mode": "default",
        "tool_name": "update_plan",
        "tool_input": {"plan": []},
        "tool_use_id": "clauseo-self-test",
    })
    .to_string()
    .into_bytes()
}

/// The hook's command line for this OS in Codex's `hooks` object (the first handler of the first
/// `PreToolUse` group, as `codex_hook_config` writes it): `command`, or `commandWindows` on
/// Windows.
pub(crate) fn command_of(entry: &Map<String, Value>) -> Option<String> {
    let key = if cfg!(windows) {
        "commandWindows"
    } else {
        "command"
    };
    let handler = entry.get("PreToolUse")?.get(0)?.get("hooks")?.get(0)?;
    Some(handler.get(key)?.as_str()?.to_owned())
}

/// Runs the hook `entry` on `input` in `cwd` with the environment `env`, as Codex runs it: `Ok`
/// when it refused (exit 2, a reason on stderr) within [`LIMIT`]. Blocks the calling thread.
pub(crate) fn run(
    entry: Option<&Map<String, Value>>,
    input: &[u8],
    cwd: &Path,
    env: &BTreeMap<OsString, OsString>,
) -> Result<(), Failed> {
    run_within(entry, (input, cwd, env), LIMIT)
}

/// Runs the hook `entry` on `input` in `cwd` with the environment `env`, as Codex runs it: `Ok`
/// when it allowed the call (exit 0, stdout empty or one JSON value) within [`LIMIT`]. Blocks the
/// calling thread.
pub(crate) fn allows(
    entry: Option<&Map<String, Value>>,
    input: &[u8],
    cwd: &Path,
    env: &BTreeMap<OsString, OsString>,
) -> Result<(), Failed> {
    allows_within(entry, (input, cwd, env), LIMIT)
}

/// The program and arguments Codex 0.162.1 starts for the hook command line `command` with the
/// environment `env` on this machine ([`argv_on` with the account's login shell and the real file
/// system).
pub(super) fn argv(command: &str, env: &BTreeMap<OsString, OsString>) -> (PathBuf, Vec<OsString>) {
    let exists = |path: &Path| path.is_file();
    let host = Host {
        os: Os::this(),
        login: login_shell(),
        exists: &exists,
    };
    argv_on(command, env, &host)
}

/// The OS a shell is picked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Os {
    Windows,
    /// macOS.
    Mac,
    /// Linux and the other Unix systems.
    Unix,
}

impl Os {
    /// The OS this runs on.
    pub(super) const fn this() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Unix
        }
    }
}

/// What [`argv_on`] picks a shell from: the OS, the account's login shell from the user database
/// (none on Windows), and whether a file is there.
pub(super) struct Host<'a> {
    pub(super) os: Os,
    pub(super) login: Option<PathBuf>,
    pub(super) exists: &'a dyn Fn(&Path) -> bool,
}

/// The shells Codex 0.162.1 knows (`shell-command/src/shell_detect.rs`, `ShellType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Zsh,
    Bash,
    Sh,
    PowerShell,
    Cmd,
}

/// The kind of the shell at `path`, as Codex's `detect_shell_type` reads it: the whole text, else
/// its file stem, again until the stem is the text itself.
pub(super) fn kind_of(path: &Path) -> Option<Kind> {
    let mut path = path.to_path_buf();
    loop {
        match path.to_str() {
            Some("zsh") => return Some(Kind::Zsh),
            Some("sh") => return Some(Kind::Sh),
            Some("cmd") => return Some(Kind::Cmd),
            Some("bash") => return Some(Kind::Bash),
            Some("pwsh" | "powershell") => return Some(Kind::PowerShell),
            _ => {}
        }
        let stem = PathBuf::from(path.file_stem()?);
        if stem == path {
            return None;
        }
        path = stem;
    }
}

/// The account's login shell from the user database (`getpwuid_r`, as Codex's
/// `get_user_shell_path`), not the environment's `SHELL`; none on Windows or when it can't be read.
fn login_shell() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        let user = nix::unistd::User::from_uid(nix::unistd::getuid()).ok()??;
        Some(user.shell).filter(|shell| !shell.as_os_str().is_empty())
    }
    #[cfg(not(unix))]
    None
}

/// The program and arguments Codex 0.162.1 starts for the hook command line `command` with the
/// environment `env` on `host`. Codex runs a hook under the session's shell
/// (`core/src/session/mod.rs:5141-5149`: the shell's `derive_exec_args("", false)` less its last
/// argument, then `hooks/src/engine/command_runner.rs:384-410`: the command line as one more
/// argument, or raw in quotes after a `/c`), and the session's shell is `default_user_shell()`
/// (`core/src/session/session.rs:1405`, `shell-command/src/shell_detect.rs`):
/// - on Windows PowerShell with `-NoProfile -Command <line>` (`core/src/shell.rs:34-43`), found
///   as `get_powershell_shell` finds it: `pwsh` on `PATH`,
///   `C:\Program Files\PowerShell\7\pwsh.exe`, `powershell` on `PATH`,
///   `C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe`; with none, `cmd.exe /c "<line>"`.
///   The Store-path skip (`fallback_powershell_shell_for_windows_sandbox`) is for sandboxed
///   commands only (`core/src/tools/runtimes/mod.rs:168`), not the session's shell;
/// - elsewhere the account's login shell (`Host::login`) when [`kind_of`] knows it, found as
///   Codex's `get_shell` finds that kind (the login shell itself when it is a file, else the
///   kind's program on `PATH`, else its fixed paths); else zsh then bash on macOS, bash then zsh
///   elsewhere; else `/bin/sh`. Run non-login: `-c <line>` for zsh, bash and sh,
///   `-NoProfile -Command <line>` for PowerShell, `/c <line>` for cmd (`core/src/shell.rs:26-49`).
///
/// `PATH` is the agent's, which Codex's process has; a program on it counts when it is a file.
#[expect(
    clippy::disallowed_methods,
    reason = "Codex looks its shell up on the agent's PATH (`shell_detect.rs`); the self-test looks \
              it up as Codex does"
)]
pub(super) fn argv_on(
    command: &str,
    env: &BTreeMap<OsString, OsString>,
    host: &Host<'_>,
) -> (PathBuf, Vec<OsString>) {
    let exists = host.exists;
    let var = |name: &str| {
        env.iter()
            .find(|(key, _)| {
                key.to_str()
                    .is_some_and(|key| key.eq_ignore_ascii_case(name))
            })
            .map(|(_, value)| value.clone())
            .filter(|value| !value.is_empty())
    };
    let on_path = |name: &str| {
        let path = var("PATH")?;
        std::env::split_paths(&path)
            .map(|folder| folder.join(name))
            .find(|program| exists(program))
    };
    let args = |kind: Kind| -> Vec<OsString> {
        let flags: &[&str] = match kind {
            Kind::Zsh | Kind::Bash | Kind::Sh => &["-c"],
            Kind::PowerShell => &["-NoProfile", "-Command"],
            Kind::Cmd => &["/c"],
        };
        flags
            .iter()
            .map(OsString::from)
            .chain([OsString::from(command)])
            .collect()
    };
    if host.os != Os::Windows {
        let (program, kind) = unix_shell(host, &on_path);
        return (program, args(kind));
    }
    let fixed = |path: &str| Some(PathBuf::from(path)).filter(|path| exists(path));
    let powershell = on_path("pwsh.exe")
        .or_else(|| fixed(r"C:\Program Files\PowerShell\7\pwsh.exe"))
        .or_else(|| on_path("powershell.exe"))
        .or_else(|| fixed(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"));
    match powershell {
        Some(program) => (program, args(Kind::PowerShell)),
        None => (
            PathBuf::from("cmd.exe"),
            vec![
                OsString::from("/c"),
                OsString::from(format!("\"{command}\"")),
            ],
        ),
    }
}

/// The session's shell off Windows and its kind, as Codex's `default_user_shell_from_path` picks
/// it on `host`, with `on_path` finding a program on the agent's `PATH` ([`argv_on`]).
fn unix_shell(host: &Host<'_>, on_path: &dyn Fn(&str) -> Option<PathBuf>) -> (PathBuf, Kind) {
    let login = host.login.as_deref();
    // Codex's `get_shell`: each program name of the kind with its fixed paths, in order.
    let find = |kind: Kind| -> Option<(PathBuf, Kind)> {
        let tries: &[(&str, &[&str])] = match kind {
            Kind::Zsh => &[("zsh", &["/bin/zsh"])],
            Kind::Bash => &[("bash", &["/bin/bash", "/usr/bin/bash"])],
            Kind::Sh => &[("sh", &["/bin/sh"])],
            Kind::PowerShell => &[("pwsh", &["/usr/local/bin/pwsh"]), ("powershell", &[])],
            Kind::Cmd => &[("cmd", &[])],
        };
        let program = tries.iter().find_map(|(name, fixed)| {
            login
                .filter(|login| kind_of(login) == Some(kind) && (host.exists)(login))
                .map(Path::to_path_buf)
                .or_else(|| on_path(name))
                .or_else(|| {
                    fixed
                        .iter()
                        .map(PathBuf::from)
                        .find(|path| (host.exists)(path))
                })
        })?;
        Some((program, kind))
    };
    let fallbacks = if host.os == Os::Mac {
        [Kind::Zsh, Kind::Bash]
    } else {
        [Kind::Bash, Kind::Zsh]
    };
    login
        .and_then(kind_of)
        .and_then(find)
        .or_else(|| fallbacks.into_iter().find_map(find))
        .unwrap_or_else(|| (PathBuf::from("/bin/sh"), Kind::Sh))
}

/// [`argv`]'s program and arguments for this OS as a command, with no window on Windows; after a
/// `/c` the last argument goes raw (`command_runner.rs:404-405`).
fn shell(command: &str, env: &BTreeMap<OsString, OsString>) -> Command {
    let (program, args) = argv(command, env);
    let mut shell = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        shell.creation_flags(CREATE_NO_WINDOW);
        let raw = args.iter().any(|arg| arg.eq_ignore_ascii_case("/c"));
        if let (true, Some((last, rest))) = (raw, args.split_last()) {
            shell.args(rest).raw_arg(last);
            return shell;
        }
    }
    shell.args(args);
    shell
}

/// What the hook answered: its exit code (none when killed by a signal), and what it wrote to
/// stderr and stdout, each read up to [`MAX_OUTPUT`].
struct Answer {
    code: Option<i32>,
    stderr: Vec<u8>,
    stdout: Vec<u8>,
}

/// [`run`] within `limit`; a run past it is killed.
pub(super) fn run_within(
    entry: Option<&Map<String, Value>>,
    call: (&[u8], &Path, &BTreeMap<OsString, OsString>),
    limit: Duration,
) -> Result<(), Failed> {
    let answer = answer_within(entry, call, limit)?;
    let said = answer.stderr.iter().any(|byte| !byte.is_ascii_whitespace());
    // The Windows line's own ending when the guard did not answer (missing, stopped, crashed):
    // exit 2 with [`UNCHECKED`](super::UNCHECKED), which is no answer of the guard's, so the
    // launch is refused as it is for a missing guard elsewhere.
    let unchecked = String::from_utf8_lossy(&answer.stderr).contains(super::UNCHECKED);
    if answer.code == Some(super::DENY) && said && !unchecked {
        Ok(())
    } else {
        tracing::warn!(
            code = ?answer.code,
            stderr = said,
            "the guard hook's self-test answered without refusing"
        );
        Err(Failed::Answer)
    }
}

/// [`allows`] within `limit`; a run past it is killed.
pub(super) fn allows_within(
    entry: Option<&Map<String, Value>>,
    call: (&[u8], &Path, &BTreeMap<OsString, OsString>),
    limit: Duration,
) -> Result<(), Failed> {
    let answer = answer_within(entry, call, limit)?;
    let quiet = answer.stdout.iter().all(u8::is_ascii_whitespace);
    let json = quiet || serde_json::from_slice::<Value>(&answer.stdout).is_ok();
    if answer.code == Some(0) && json {
        Ok(())
    } else {
        tracing::warn!(
            code = ?answer.code,
            stdout_json = json,
            "the guard hook's self-test did not allow the conversation's own call"
        );
        Err(Failed::Refused)
    }
}

/// Reads `pipe` to its end, up to [`MAX_OUTPUT`], on a thread of its own.
fn reader(pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut text = Vec::new();
        drop(pipe.take(MAX_OUTPUT).read_to_end(&mut text));
        text
    })
}

/// Runs the hook `entry` on the call (its input, working folder and environment) within `limit`,
/// as Codex runs it; a run past it is killed.
#[expect(
    clippy::disallowed_methods,
    reason = "a one-shot child on its own off-actor thread, started as Codex starts its hook \
              (std's Command, `raw_arg` on Windows); its limit is in real time"
)]
fn answer_within(
    entry: Option<&Map<String, Value>>,
    (input, cwd, env): (&[u8], &Path, &BTreeMap<OsString, OsString>),
    limit: Duration,
) -> Result<Answer, Failed> {
    let command = entry.and_then(command_of).ok_or(Failed::NoEntry)?;
    let start = Instant::now();
    let mut child = shell(&command, env)
        .env_clear()
        .envs(env)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_not_started| Failed::Launch)?;
    // Each pipe on a thread of its own, so a guard that reads nothing or writes much never holds
    // up the limit.
    let input = input.to_vec();
    let stdin = child
        .stdin
        .take()
        .map(|mut stdin| std::thread::spawn(move || drop(stdin.write_all(&input))));
    let stderr = child.stderr.take().map(reader);
    let stdout = child.stdout.take().map(reader);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(5)),
            Ok(None) | Err(_) => break None,
        }
    };
    let Some(status) = status else {
        drop(child.kill());
        drop(child.wait());
        return Err(Failed::Late);
    };
    drop(stdin.map(std::thread::JoinHandle::join));
    // The guard is done; a grandchild holding a pipe open could keep it, so each read gets what
    // is left of the limit.
    let collect = |reader: Option<std::thread::JoinHandle<Vec<u8>>>| {
        reader
            .map(|reader| {
                while !reader.is_finished() && start.elapsed() < limit {
                    std::thread::sleep(Duration::from_millis(5));
                }
                if reader.is_finished() {
                    reader.join().unwrap_or_default()
                } else {
                    Vec::new()
                }
            })
            .unwrap_or_default()
    };
    Ok(Answer {
        code: status.code(),
        stderr: collect(stderr),
        stdout: collect(stdout),
    })
}

#[cfg(test)]
#[path = "self_test/tests.rs"]
mod tests;
