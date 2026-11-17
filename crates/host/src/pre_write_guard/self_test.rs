//! The launch's self-test of Codex's guard hook: before a Codex session's first prompt, the host
//! runs the hook's configured command line as Codex 0.162.1 runs it (under the session's shell,
//! [`argv`]: PowerShell `-NoProfile -Command <line>` on Windows, the user's shell `-c <line>`
//! elsewhere; the environment cleared, then the agent's; the session folder as the working
//! folder), with a call from a session no conversation has ([`input`]). The launch goes on only
//! when the guard refuses it: exit 2 with something on stderr, within [`LIMIT`]. Anything else (no
//! guard at that path, one an antivirus stops, one that hangs, crashes or lets the call go)
//! refuses the launch (`agent.guard_selftest_failed`), since Codex would let every call of that
//! session run unchecked; so does exit 2 with [`UNCHECKED`](super::UNCHECKED), the Windows
//! line's own ending when the guard did not answer (`agent/loads/codex.rs`, `ps_command`), which
//! blocks every call but shows no guard ran. Blocking: run off the actor
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
    /// [`run`] on this hook with [`input`].
    pub(crate) fn run(&self) -> Result<(), Failed> {
        run(Some(&self.entry), &input(&self.cwd), &self.cwd, &self.env)
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

/// The program and arguments Codex 0.162.1 starts for the hook command line `command` with the
/// environment `env`, on Windows when `windows`. Codex runs a hook under the session's shell
/// (`core/src/session/mod.rs:5141-5149`: the shell's `derive_exec_args("", false)` less its last
/// argument, then `hooks/src/engine/command_runner.rs:384-410`: the command line as one more
/// argument, or raw in quotes after a `/c`), and the session's shell is `default_user_shell()`
/// (`core/src/session/session.rs:1405`):
/// - on Windows PowerShell with `-NoProfile -Command <line>` (`core/src/shell.rs:34-43`), found
///   as `get_powershell_shell` finds it (`shell-command/src/shell_detect.rs:192-216, 255-281`):
///   `pwsh` on `PATH`, `C:\Program Files\PowerShell\7\pwsh.exe`, `powershell` on `PATH`,
///   `C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe`; with none, `cmd.exe /c
///   "<line>"` (`shell_detect.rs:315-320`, `core/src/shell.rs:44-49`). The Store-path skip
///   (`fallback_powershell_shell_for_windows_sandbox`) is for sandboxed commands only
///   (`core/src/tools/runtimes/mod.rs:168`), not the session's shell, so not for hooks;
/// - elsewhere the user's shell with `-c <line>` (`core/src/shell.rs:26-33`), its path here from
///   the agent's `SHELL`, else `/bin/sh`.
///
/// After a `/c` the last argument is passed raw, as Codex passes it (`shell`).
pub(super) fn argv(
    command: &str,
    env: &BTreeMap<OsString, OsString>,
    windows: bool,
) -> (PathBuf, Vec<OsString>) {
    argv_on(command, env, windows, &|path: &Path| path.is_file())
}

/// [`argv`], with `exists` saying whether a file is there.
#[expect(
    clippy::disallowed_methods,
    reason = "Codex looks PowerShell up on the agent's PATH (`shell_detect.rs:205`); the \
              self-test looks it up as Codex does"
)]
pub(super) fn argv_on(
    command: &str,
    env: &BTreeMap<OsString, OsString>,
    windows: bool,
    exists: &dyn Fn(&Path) -> bool,
) -> (PathBuf, Vec<OsString>) {
    let var = |name: &str| {
        env.iter()
            .find(|(key, _)| {
                key.to_str()
                    .is_some_and(|key| key.eq_ignore_ascii_case(name))
            })
            .map(|(_, value)| value.clone())
            .filter(|value| !value.is_empty())
    };
    if !windows {
        let program = var("SHELL").map_or_else(|| PathBuf::from("/bin/sh"), PathBuf::from);
        return (program, vec![OsString::from("-c"), OsString::from(command)]);
    }
    let on_path = |name: &str| {
        let path = var("PATH")?;
        std::env::split_paths(&path)
            .map(|folder| folder.join(name))
            .find(|program| exists(program))
    };
    let fixed = |path: &str| Some(PathBuf::from(path)).filter(|path| exists(path));
    let powershell = on_path("pwsh.exe")
        .or_else(|| fixed(r"C:\Program Files\PowerShell\7\pwsh.exe"))
        .or_else(|| on_path("powershell.exe"))
        .or_else(|| fixed(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"));
    match powershell {
        Some(program) => {
            let args = ["-NoProfile", "-Command", command].map(OsString::from);
            (program, args.to_vec())
        }
        None => (
            PathBuf::from("cmd.exe"),
            vec![
                OsString::from("/c"),
                OsString::from(format!("\"{command}\"")),
            ],
        ),
    }
}

/// [`argv`]'s program and arguments for this OS as a command, with no window on Windows; after a
/// `/c` the last argument goes raw (`command_runner.rs:404-405`).
fn shell(command: &str, env: &BTreeMap<OsString, OsString>) -> Command {
    let (program, args) = argv(command, env, cfg!(windows));
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

/// [`run`] within `limit`; a run past it is killed.
#[expect(
    clippy::disallowed_methods,
    reason = "a one-shot child on its own off-actor thread, started as Codex starts its hook \
              (std's Command, `raw_arg` on Windows); its limit is in real time"
)]
pub(super) fn run_within(
    entry: Option<&Map<String, Value>>,
    (input, cwd, env): (&[u8], &Path, &BTreeMap<OsString, OsString>),
    limit: Duration,
) -> Result<(), Failed> {
    let command = entry.and_then(command_of).ok_or(Failed::NoEntry)?;
    let start = Instant::now();
    let mut child = shell(&command, env)
        .env_clear()
        .envs(env)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
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
    let stderr = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            drop(stderr.take(MAX_OUTPUT).read_to_end(&mut text));
            text
        })
    });
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
    // The guard is done; a grandchild holding stderr open could keep the pipe, so the read gets
    // what is left of the limit.
    let text = stderr
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
        .unwrap_or_default();
    let said = text.iter().any(|byte| !byte.is_ascii_whitespace());
    // The Windows line's own ending when the guard did not answer (missing, stopped, crashed):
    // exit 2 with [`UNCHECKED`](super::UNCHECKED), which blocks every call but is no answer of
    // the guard's, so the launch is refused as it is for a missing guard elsewhere.
    let unchecked = String::from_utf8_lossy(&text).contains(super::UNCHECKED);
    if status.code() == Some(super::DENY) && said && !unchecked {
        Ok(())
    } else {
        tracing::warn!(
            code = ?status.code(),
            stderr = said,
            "the guard hook's self-test answered without refusing"
        );
        Err(Failed::Answer)
    }
}

#[cfg(test)]
#[path = "self_test/tests.rs"]
mod tests;
