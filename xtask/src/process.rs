//! The only module that starts processes. Every other module goes through `capture`, `run_quiet`
//! or `run_timed`, so the rules about how commands run live in one place.

use std::ffi::OsStr;
use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

/// Size of each read from a running command's output pipe.
const CHUNK: usize = 8 * 1024;

/// The program a command runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Program {
    /// The cargo that started this xtask (the `CARGO` env var), falling back to `cargo`, with
    /// `CLIPPY_CONF_DIR` removed, so clippy reads the `clippy.toml` files core-io checks.
    Cargo,
    /// A program found on `PATH`.
    Named(&'static str),
    /// pnpm. On Windows it may be `pnpm.cmd` (npm and corepack install that), so it is resolved
    /// by `pnpm_file`.
    Pnpm,
    /// git, with the repository-local variables in `GIT_LOCAL_ENV` removed, so it finds the repo
    /// from its working directory. Git hooks export some of them (`GIT_DIR`, `GIT_INDEX_FILE`),
    /// and a test that runs git in a scratch dir from inside a hook would otherwise act on the
    /// real repo.
    Git,
}

/// The variable that points clippy at a config folder of its choosing, removed for every cargo
/// command xtask runs.
const CLIPPY_CONF_DIR: &str = "CLIPPY_CONF_DIR";

/// The variables `git rev-parse --local-env-vars` lists: each points git at a particular
/// repository, index or object store instead of the one found from the working dir. It is git
/// 2.54's list plus `GIT_SUPER_PREFIX`, which gits before 2.40 still list. A git that lists a name
/// missing here fails `every_variable_the_local_git_lists_is_in_git_local_env`; a git that lists
/// fewer passes.
const GIT_LOCAL_ENV: [&str; 16] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SUPER_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

impl Program {
    /// The name people see: `cargo`, the named program, or `pnpm`.
    pub(crate) fn name(&self) -> &'static str {
        match *self {
            Program::Cargo => "cargo",
            Program::Named(name) => name,
            Program::Pnpm => "pnpm",
            Program::Git => "git",
        }
    }

    fn command(&self) -> Command {
        match *self {
            Program::Cargo => {
                let mut cargo =
                    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
                // clippy reads its config from this folder when it is set, instead of the
                // crate's clippy.toml that core-io checks.
                cargo.env_remove(CLIPPY_CONF_DIR);
                cargo
            }
            Program::Named(name) => Command::new(name),
            Program::Pnpm => Command::new(pnpm_file()),
            Program::Git => {
                let mut git = Command::new("git");
                for name in GIT_LOCAL_ENV {
                    git.env_remove(name);
                }
                git
            }
        }
    }
}

/// The file names pnpm may have on Windows, in the order they are looked for within one
/// directory: `.exe` before `.cmd`, as in Windows' default `PATHEXT` (`.COM;.EXE;.BAT;.CMD`).
const WINDOWS_PNPM: [&str; 2] = ["pnpm.exe", "pnpm.cmd"];

/// The program to start for pnpm. On Windows it is the file that cmd and PowerShell would run with
/// the default `PATHEXT`: in the first directory on `PATH` that holds `pnpm.exe` or `pnpm.cmd`,
/// the `.exe` if both are there, as a full path. A `PATHEXT` changed from the default may pick
/// differently. `Command` then runs that file without searching `PATH` again. Rust 1.77+ runs a
/// `.cmd` file with its arguments escaped, so they reach the script unchanged. Elsewhere it is
/// `pnpm`.
fn pnpm_file() -> PathBuf {
    if cfg!(windows) {
        let path = std::env::var_os("PATH").unwrap_or_default();
        resolve_pnpm(&path, &WINDOWS_PNPM, Path::is_file)
    } else {
        PathBuf::from("pnpm")
    }
}

/// The pnpm file in the first directory on `path` that holds one of `names`, with `exists` saying
/// whether a path is a file. Directories are walked in PATH order, and within one directory the
/// names are tried in the order given. So a `pnpm.cmd` in an earlier directory beats a `pnpm.exe`
/// in a later one, the same as the developer's shell. The result is `dir.join(name)`. When no
/// directory holds a match, it is the bare `pnpm`.
fn resolve_pnpm(path: &OsStr, names: &[&str], exists: impl Fn(&Path) -> bool) -> PathBuf {
    for dir in std::env::split_paths(path) {
        for name in names {
            let file = dir.join(name);
            if exists(&file) {
                return file;
            }
        }
    }
    PathBuf::from("pnpm")
}

/// A program and its arguments. Displays as the shell-like form: `cargo fmt --all -- --check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cmd {
    pub program: Program,
    pub args: Vec<String>,
}

impl Cmd {
    pub(crate) fn new<I, S>(program: Program, args: I) -> Cmd
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Cmd {
            program,
            args: args
                .into_iter()
                .map(|arg| arg.as_ref().to_owned())
                .collect(),
        }
    }
}

impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.program.name())?;
        self.args.iter().try_for_each(|arg| write!(f, " {arg}"))
    }
}

/// Exit status and merged stdout and stderr of a command run by `capture`.
pub(crate) struct Captured {
    pub success: bool,
    pub output: String,
}

/// Exit status and separate stdout and stderr of a command run by `run_quiet`.
#[derive(Debug)]
pub(crate) struct Quiet {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `cmd` in `root` with stdout and stderr merged into one stream. Each chunk read is passed
/// to `on_output` as it arrives, and kept for the returned `output`.
#[expect(clippy::disallowed_methods, reason = "xtask runs tools and times them")]
pub(crate) fn capture(
    cmd: &Cmd,
    root: &Path,
    on_output: &mut dyn FnMut(&[u8]),
) -> io::Result<Captured> {
    let (mut reader, writer) = std::io::pipe()?;
    let mut command = cmd.program.command();
    command
        .args(&cmd.args)
        .current_dir(root)
        .stdout(writer.try_clone()?)
        .stderr(writer);
    let mut child = command.spawn()?;
    // The command still holds the write end of the pipe; drop it so the reader sees EOF.
    drop(command);

    let mut bytes = Vec::new();
    let mut chunk = vec![0u8; CHUNK];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                bytes.extend_from_slice(&chunk[..n]);
                on_output(&chunk[..n]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    let status = child.wait()?;
    Ok(Captured {
        success: status.success(),
        output: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// Runs `cmd` in `root` and returns its stdout and stderr separately.
#[expect(clippy::disallowed_methods, reason = "xtask runs tools and times them")]
pub(crate) fn run_quiet(cmd: &Cmd, root: &Path) -> io::Result<Quiet> {
    let output = cmd
        .program
        .command()
        .args(&cmd.args)
        .current_dir(root)
        .output()?;
    Ok(Quiet {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Where the output of a command run by `run_timed` goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Output {
    /// Straight to the terminal. The command stays in the terminal's process group, so Ctrl-C
    /// stops it as it stops xtask.
    Inherit,
    /// Kept, stdout and stderr apart, with stdin closed. On Unix the command runs in a process
    /// group of its own, so a timeout kills everything it started.
    Quiet,
}

/// How a command run by `run_timed` ended.
#[derive(Debug)]
pub(crate) enum Ended {
    /// It exited. With `Output::Inherit`, `stdout` and `stderr` are empty.
    Exited(Quiet),
    /// It was still running at the timeout, so it was killed.
    TimedOut,
}

/// How long `run_timed` waits, after killing a command, for its output pipes to close.
const KILL_GRACE: Duration = Duration::from_secs(5);

/// Runs `cmd` in `root` and kills it if it runs longer than `timeout`. On a timeout it kills, on
/// Unix, the command's process group with `Output::Quiet` and the command alone with
/// `Output::Inherit`; on Windows, the command and every process it started (`taskkill /T`).
#[expect(clippy::disallowed_methods, reason = "xtask runs tools and times them")]
pub(crate) fn run_timed(
    cmd: &Cmd,
    root: &Path,
    output: Output,
    timeout: Duration,
) -> io::Result<Ended> {
    let mut command = cmd.program.command();
    command.args(&cmd.args).current_dir(root);
    if output == Output::Quiet {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
    }
    let child = command.spawn()?;
    let pid = child.id();
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        // The receiver is gone only after a timeout, when nobody wants the output any more.
        drop(done.send(child.wait_with_output()));
    });
    match finished.recv_timeout(timeout) {
        Ok(ran) => Ok(Ended::Exited(quiet_of(&ran?))),
        Err(RecvTimeoutError::Timeout) => {
            // A command that exited just at the timeout is not killed: its status is here already.
            if let Ok(ran) = finished.try_recv() {
                return Ok(Ended::Exited(quiet_of(&ran?)));
            }
            // What is left is the few instructions between the waiter reaping the command and
            // sending its status, where the pid is free again. macOS and Linux hand out pids in
            // rising order and wrap only after the whole range, so no new process gets it then.
            // Windows reuses pids sooner, so there a new process would have to start and take that
            // exact pid within those instructions for taskkill to hit it; that risk is accepted.
            let killed = kill(pid, output);
            // The waiter ends once the killed processes have closed the pipes. A process that left
            // the group may hold them longer; it is not waited for past the grace period.
            let late = finished.recv_timeout(KILL_GRACE);
            match (killed, late) {
                (Ok(()), _) => Ok(Ended::TimedOut),
                // The kill found nothing to kill: the command exited just at the timeout.
                (Err(_), Ok(Ok(ran))) => Ok(Ended::Exited(quiet_of(&ran))),
                (Err(error), _) => Err(io::Error::other(format!(
                    "`{cmd}` ran past its timeout of {} s and could not be stopped: {error}",
                    timeout.as_secs()
                ))),
            }
        }
        Err(RecvTimeoutError::Disconnected) => Err(io::Error::other(format!(
            "the thread waiting for `{cmd}` stopped without its exit status"
        ))),
    }
}

/// The exit status and output of a finished command.
fn quiet_of(ran: &std::process::Output) -> Quiet {
    Quiet {
        success: ran.status.success(),
        stdout: String::from_utf8_lossy(&ran.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&ran.stderr).into_owned(),
    }
}

/// The command that kills what `run_timed` started as `pid`: see `run_timed` for what each
/// `output` and OS kills.
fn kill_cmd(pid: u32, output: Output) -> Cmd {
    if cfg!(windows) {
        Cmd::new(
            Program::Named("taskkill"),
            ["/PID".to_owned(), pid.to_string(), "/T".into(), "/F".into()],
        )
    } else if output == Output::Quiet {
        // A negative pid names the process group the command leads.
        Cmd::new(Program::Named("kill"), ["-KILL", "--", &format!("-{pid}")])
    } else {
        Cmd::new(
            Program::Named("kill"),
            ["-KILL".to_owned(), pid.to_string()],
        )
    }
}

/// Runs `kill_cmd`. Fails when the kill could not start or exited non-zero.
fn kill(pid: u32, output: Output) -> io::Result<()> {
    let cmd = kill_cmd(pid, output);
    let killed = run_quiet(&cmd, Path::new("."))?;
    if killed.success {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "`{cmd}` failed: {}",
            killed.stderr.trim()
        )))
    }
}

#[cfg(test)]
mod tests;
