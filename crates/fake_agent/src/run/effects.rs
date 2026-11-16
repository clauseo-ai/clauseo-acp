//! Steps that act outside stdout: files under the cwd, the `SessionStart` canary's nonce file,
//! stderr lines, the grandchild, and the `--pace-ms` pause.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::diagnostic;
use crate::scenario::{CanaryRun, FileOp};

/// Writes `text` to stderr in one call.
pub(super) fn write_stderr(text: &str) -> std::io::Result<()> {
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(text.as_bytes())?;
    stderr.flush()
}

/// Sleeps for `duration`: the `--pace-ms` pause between updates, for the speed scoreboard's
/// session only, and the `FAKE_SIGNIN=ok-after:<ms>` wait of a sign-in mode (`signin.rs`).
/// Scenarios themselves stay timer-free.
#[expect(
    clippy::disallowed_methods,
    reason = "a fake pauses for --pace-ms or ok-after; scenarios stay timer-free"
)]
pub(crate) fn pause(duration: Duration) {
    std::thread::sleep(duration);
}

/// Writes `line` to stderr `repeat` times, in one write.
pub(super) fn stderr_step(line: &str, repeat: u32) -> std::io::Result<()> {
    let mut text = String::with_capacity((line.len() + 1).saturating_mul(repeat as usize));
    for _ in 0..repeat {
        text.push_str(line);
        text.push('\n');
    }
    write_stderr(&text)
}

/// Runs a `write_file`, `delete_file` or `move_file` step under `cwd`. Paths were checked to be
/// relative at load.
pub(super) fn file_step(cwd: &str, op: &FileOp) -> Result<(), String> {
    let cwd = Path::new(cwd);
    match op {
        FileOp::Write { path, content } => {
            let full = cwd.join(path);
            make_parent(&full)
                .and_then(|()| crate::write::write(&full, content))
                .map_err(|e| format!("write_file {path}: {e}"))
        }
        FileOp::Delete { path } => {
            let full = cwd.join(path);
            if !full.is_file() {
                return Err(format!("no such file {path}"));
            }
            crate::write::remove_file(&full).map_err(|e| format!("delete_file {path}: {e}"))
        }
        FileOp::Move { from, to } => {
            let source = cwd.join(from);
            if !source.is_file() {
                return Err(format!("no such file {from}"));
            }
            let target = cwd.join(to);
            make_parent(&target)
                .and_then(|()| crate::write::rename(&source, &target))
                .map_err(|e| format!("move_file {from} to {to}: {e}"))
        }
    }
}

/// The arguments that start Clauseo's `SessionStart` canary hook: the guard program's flag, then
/// its canary flag, then the nonce file and the nonce.
const CANARY_ARGS: [&str; 2] = ["--pre-write-guard", "--session-canary"];

/// Runs a `session_canary` step over `session_start`, the last session call's
/// `hooks.SessionStart`: finds the command hook whose `args` are [`CANARY_ARGS`], a file and a
/// nonce, and writes the nonce (`run`) or another value (`wrong`) to that file, as Clauseo's
/// canary would; the program is never run. No such hook, or a file that is not absolute, fails.
pub(super) fn session_canary(session_start: Option<&Value>, run: CanaryRun) -> Result<(), String> {
    let hooks = session_start
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("hooks")?.as_array())
        .flatten();
    let found = hooks
        .filter_map(|hook| hook.get("args")?.as_array())
        .find_map(|args| {
            let args: Vec<&str> = args.iter().map(Value::as_str).collect::<Option<_>>()?;
            match args.as_slice() {
                [flag, canary, file, nonce] if [*flag, *canary] == CANARY_ARGS => {
                    Some((*file, *nonce))
                }
                _ => None,
            }
        });
    let Some((file, nonce)) = found else {
        return Err(
            "session_canary: the last session call's flag settings hold no SessionStart canary"
                .to_owned(),
        );
    };
    if !Path::new(file).is_absolute() {
        return Err(format!(
            "session_canary: the nonce file {file} is not absolute"
        ));
    }
    let content = match run {
        CanaryRun::Run => nonce.to_owned(),
        CanaryRun::Wrong => format!("{nonce}0"),
    };
    crate::write::write(file, content).map_err(|e| format!("session_canary: write {file}: {e}"))
}

fn make_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(parent) => crate::write::create_dir_all(parent),
        None => Ok(()),
    }
}

/// Starts `fake_agent --hold` with null stdio (with `own_group`, in a process group of its own
/// on Unix), writes its PID to `pid_file` (relative to the process folder, the agent's working
/// directory, not the session `cwd`), then writes `fake_agent: child <pid>` to stderr.
pub(super) fn spawn_child(pid_file: &str, own_group: bool) -> Result<(), String> {
    let pid = start_hold(own_group).map_err(|e| format!("spawn_child: {e}"))?;
    crate::write::write(pid_file, format!("{pid}\n"))
        .map_err(|e| format!("spawn_child: write {pid_file}: {e}"))?;
    write_stderr(&diagnostic(format_args!("child {pid}")))
        .map_err(|e| format!("write to stderr failed: {e}"))
}

#[expect(
    clippy::disallowed_methods,
    reason = "the one process fake_agent starts: the grandchild S7's tree-kill tests need"
)]
fn start_hold(own_group: bool) -> std::io::Result<u32> {
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg("--hold")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // Out of the agent's process group, as Codex starts each command in a session of its own:
    // a signal to the agent's group no longer reaches it. Windows has no groups: the child stays
    // in the agent's job, as a Codex command does.
    #[cfg(unix)]
    if own_group {
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
    }
    #[cfg(not(unix))]
    let _: bool = own_group;
    let child = command.spawn()?;
    // Never waited for: the grandchild must outlive this process until a test kills it.
    Ok(child.id())
}
