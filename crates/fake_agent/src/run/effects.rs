//! Steps that act outside stdout: files under the cwd, stderr lines, and the grandchild.

use std::io::Write;
use std::path::Path;

use crate::diagnostic;
use crate::scenario::FileOp;

/// Writes `text` to stderr in one call.
pub(super) fn write_stderr(text: &str) -> std::io::Result<()> {
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(text.as_bytes())?;
    stderr.flush()
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

fn make_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(parent) => crate::write::create_dir_all(parent),
        None => Ok(()),
    }
}

/// Starts `fake_agent --hold` with null stdio, writes its PID to `pid_file` (relative to the
/// process folder, the agent's working directory, not the session `cwd`), then writes
/// `fake_agent: child <pid>` to stderr.
pub(super) fn spawn_child(pid_file: &str) -> Result<(), String> {
    let pid = start_hold().map_err(|e| format!("spawn_child: {e}"))?;
    crate::write::write(pid_file, format!("{pid}\n"))
        .map_err(|e| format!("spawn_child: write {pid_file}: {e}"))?;
    write_stderr(&diagnostic(format_args!("child {pid}")))
        .map_err(|e| format!("write to stderr failed: {e}"))
}

#[expect(
    clippy::disallowed_methods,
    reason = "the one process fake_agent starts: the grandchild S7's tree-kill tests need"
)]
fn start_hold() -> std::io::Result<u32> {
    let child = std::process::Command::new(std::env::current_exe()?)
        .arg("--hold")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    // Never waited for: the grandchild must outlive this process until a test kills it.
    Ok(child.id())
}
