//! The only module that starts processes. Every other module goes through `capture`, `run_quiet`
//! or `run`, and those three through `running.rs` and the one `spawn` here, so the rules about how
//! commands run live in one place:
//! - A command cannot run without a time limit: `Cmd::new` takes it.
//! - It runs with stdin closed, in a process tree of its own: a new process group on Unix, a Job
//!   Object that kills its processes when its handle closes on Windows (`windows.rs`).
//! - Its tree is stopped at the limit and when its run is dropped (a panic included): on Unix
//!   SIGTERM to the group, then SIGKILL after `KILL_GRACE`; on Windows `TerminateJobObject`, which
//!   ends every process in the job at once. What is left of the tree when the command exits gets
//!   SIGKILL on Unix, and is ended with the job on Windows (`running.rs`).
//! - Unix: it is also stopped when xtask dies, however it dies, by the leader (`leader.rs`).
//!   Ctrl-C reaches only xtask, since each command has its own group; xtask dies of it, and the
//!   leader then stops every group still running.
//! - Windows: there is no leader. When xtask dies, however it dies, Windows closes its handles,
//!   and closing a job's handle ends every process in it. Ctrl-C reaches every process attached to
//!   xtask's console, so a command that has not detached from it gets Ctrl-C at the same time as
//!   xtask. Not run on Windows yet.

#[cfg(unix)]
mod leader;
mod running;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use leader::lead;

use running::{Finish, Stream, Streams};
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

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
    first_on_path(path, |dir| {
        names
            .iter()
            .map(|name| dir.join(name))
            .find(|file| exists(file))
    })
    .unwrap_or_else(|| PathBuf::from("pnpm"))
}

/// The first `found(dir)` that is `Some`, for the directories `path` lists, in PATH order. The one
/// walk over PATH in xtask: `resolve_pnpm` looks pnpm up with it, and setup asks whether a
/// directory is on PATH. A second lookup reuses it instead of splitting PATH again.
#[expect(
    clippy::disallowed_methods,
    reason = "the one walk over PATH; clippy.toml bans split_paths everywhere else"
)]
pub(crate) fn first_on_path<T>(
    path: &OsStr,
    mut found: impl FnMut(&Path) -> Option<T>,
) -> Option<T> {
    std::env::split_paths(path).find_map(|dir| found(&dir))
}

/// A program, its arguments and how long it may run. Displays as the shell-like form:
/// `cargo fmt --all -- --check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cmd {
    pub program: Program,
    pub args: Vec<String>,
    /// How long it may run before its tree is stopped. Every call site passes a named `const`
    /// that says why that long.
    pub timeout: Duration,
}

impl Cmd {
    pub(crate) fn new<I, S>(program: Program, args: I, timeout: Duration) -> Cmd
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
            timeout,
        }
    }
}

impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.program.name())?;
        self.args.iter().try_for_each(|arg| write!(f, " {arg}"))
    }
}

/// Starts `command`. The one call of `Command::spawn` in xtask: `running.rs` starts every command
/// through it, and `leader.rs` the leader. Nothing calls `Command::output` or `Command::status`.
#[expect(
    clippy::disallowed_methods,
    reason = "the process door: every child xtask starts goes through here"
)]
fn spawn(command: &mut Command) -> io::Result<Child> {
    command.spawn()
}

/// Exit status and merged stdout and stderr of a command run by `capture`.
#[derive(Debug)]
pub(crate) struct Captured {
    pub success: bool,
    pub output: String,
}

/// Exit status and separate stdout and stderr of a command run by `run_quiet` or `run`.
#[derive(Debug)]
pub(crate) struct Quiet {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `cmd` in `root` with stdout and stderr merged into one stream. Each chunk read is passed
/// to `on_output` as it arrives, and kept for the returned `output`. A command that runs past its
/// limit is an error of kind `TimedOut` (see `timed_out`). A command that exits while a process
/// that left its tree still holds its output is an error that says so, after `running::DRAIN`.
pub(crate) fn capture(
    cmd: &Cmd,
    root: &Path,
    on_output: &mut dyn FnMut(&[u8]),
) -> io::Result<Captured> {
    let mut bytes = Vec::new();
    let finish = running::run(cmd, root, Streams::Merged, &mut |_, chunk| {
        bytes.extend_from_slice(chunk);
        on_output(chunk);
    })?;
    match finish {
        Finish::Exited(status) => Ok(Captured {
            success: status.success(),
            output: String::from_utf8_lossy(&bytes).into_owned(),
        }),
        Finish::TimedOut => Err(timed_out(cmd)),
    }
}

/// Runs `cmd` in `root` and returns its stdout and stderr separately. A command that runs past
/// its limit is an error of kind `TimedOut` (see `timed_out`); one whose output a process that
/// left its tree still holds is an error, as for `capture`.
pub(crate) fn run_quiet(cmd: &Cmd, root: &Path) -> io::Result<Quiet> {
    match run(cmd, root, Output::Quiet)? {
        Ended::Exited(quiet) => Ok(quiet),
        Ended::TimedOut => Err(timed_out(cmd)),
    }
}

/// Where the output of a command run by `run` goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Output {
    /// Straight to xtask's stdout and stderr, so the terminal when xtask has one. On Unix the
    /// command still runs in a process group of its own: Ctrl-C reaches only xtask, and the leader
    /// stops the command when xtask dies of it. On Windows it runs in its own job and shares
    /// xtask's console, so Ctrl-C reaches it as well as xtask, and Windows ends the job when xtask
    /// dies.
    Inherit,
    /// Kept, stdout and stderr apart.
    Quiet,
}

/// How a command run by `run` ended.
#[derive(Debug)]
pub(crate) enum Ended {
    /// It exited. With `Output::Inherit`, `stdout` and `stderr` are empty.
    Exited(Quiet),
    /// It was still running at its limit, so its tree was stopped.
    TimedOut,
}

/// Runs `cmd` in `root` with its output going where `output` says, and stops its tree if it runs
/// past its limit. With `Output::Quiet`, output a process that left its tree still holds is an
/// error, as for `capture`.
pub(crate) fn run(cmd: &Cmd, root: &Path, output: Output) -> io::Result<Ended> {
    let streams = match output {
        Output::Inherit => Streams::Inherit,
        Output::Quiet => Streams::Separate,
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let finish = running::run(cmd, root, streams, &mut |stream, chunk| match stream {
        Stream::Out => stdout.extend_from_slice(chunk),
        Stream::Err => stderr.extend_from_slice(chunk),
    })?;
    Ok(match finish {
        Finish::Exited(status) => Ended::Exited(Quiet {
            success: status.success(),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }),
        Finish::TimedOut => Ended::TimedOut,
    })
}

/// The error `capture` and `run_quiet` return for `cmd` when it runs past its limit. Its kind is
/// `TimedOut`, which no start or read error has, so `is_timeout` tells it apart.
pub(crate) fn timed_out(cmd: &Cmd) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "`{cmd}` did not finish within {}, so it was stopped",
            limit_text(cmd.timeout)
        ),
    )
}

/// Whether `error`, from `capture` or `run_quiet`, is the command running past its limit.
pub(crate) fn is_timeout(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::TimedOut
}

/// The fix of a command that ran past its limit, as text, for the steps and buildtime.
pub(crate) fn timeout_fix(cmd: &Cmd) -> String {
    TimeoutFix::of(cmd).to_string()
}

/// The fix of a command that ran past its limit: run it by hand. The one home of its wording, which
/// a check's typed `Fix` wraps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimeoutFix {
    cmd: String,
    limit: String,
}

impl TimeoutFix {
    /// The fix for `cmd`, which ran past its own limit.
    pub(crate) fn of(cmd: &Cmd) -> TimeoutFix {
        TimeoutFix {
            cmd: cmd.to_string(),
            limit: limit_text(cmd.timeout),
        }
    }
}

impl fmt::Display for TimeoutFix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "run `{}` by hand to see why it does not finish within {}",
            self.cmd, self.limit
        )
    }
}

/// `limit` in the largest unit that divides it: `1 h`, `30 min`, `30 s` or `500 ms`.
pub(crate) fn limit_text(limit: Duration) -> String {
    let secs = limit.as_secs();
    if limit.subsec_nanos() != 0 || secs == 0 {
        format!("{} ms", limit.as_millis())
    } else if secs.is_multiple_of(3600) {
        format!("{} h", secs / 3600)
    } else if secs.is_multiple_of(60) {
        format!("{} min", secs / 60)
    } else {
        format!("{secs} s")
    }
}

/// The limit of the commands tests run (git init, tar, ps, short sh scripts), and of the commands
/// they only build or display: each runs well under a second, so a minute means it hangs.
#[cfg(test)]
pub(crate) const TEST_TIMEOUT: Duration = Duration::from_mins(1);

/// How the hidden `process-test` mode runs its script, for xtask/tests/process.rs.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum TestOutput {
    /// Through `run` with `Output::Inherit`.
    Inherit,
    /// Through `capture`.
    Capture,
    /// Started with its output inherited; then, on Unix, the leader is killed, and the script runs
    /// again to its end through `run`, with `$1` set to `again` (which starts a new leader); then
    /// the first run is waited for. So only a new leader told about the first run's group can stop
    /// it when xtask dies.
    RestartedLeader,
}

/// The limit of the hidden `process-test` mode: longer than any test waits, so the test's signal
/// ends the run, not the limit.
const PROCESS_TEST_TIMEOUT: Duration = Duration::from_mins(10);

/// The hidden `process-test` mode: runs `sh -c <script>` through the door as `output` says, so
/// xtask/tests/process.rs can kill this xtask while the script runs. Exits 0 when the script
/// exits 0, 1 otherwise.
pub(crate) fn test_mode(output: TestOutput, script: &str) -> std::process::ExitCode {
    let cmd = Cmd::new(Program::Named("sh"), ["-c", script], PROCESS_TEST_TIMEOUT);
    let success = match output {
        TestOutput::Inherit => matches!(
            run(&cmd, Path::new("."), Output::Inherit),
            Ok(Ended::Exited(Quiet { success: true, .. }))
        ),
        TestOutput::Capture => matches!(
            capture(&cmd, Path::new("."), &mut |_| {}),
            Ok(Captured { success: true, .. })
        ),
        TestOutput::RestartedLeader => restarted_leader(&cmd, script),
    };
    if success {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

/// The `restarted-leader` test mode for `cmd`, which is `sh -c <script>`. Whether both runs
/// exited 0.
fn restarted_leader(cmd: &Cmd, script: &str) -> bool {
    let mut command = cmd.program.command();
    command.args(&cmd.args);
    let limits = running::Limits::of(cmd.timeout);
    let started = running::Running::start(command, &cmd.to_string(), Streams::Inherit, limits);
    let Ok(first) = started else {
        return false;
    };
    #[cfg(unix)]
    leader::kill_for_test();
    let again = Cmd::new(
        Program::Named("sh"),
        ["-c", script, "sh", "again"],
        cmd.timeout,
    );
    let second = run(&again, Path::new("."), Output::Inherit);
    let first = first.finish(&mut |_, _| {});
    matches!(second, Ok(Ended::Exited(Quiet { success: true, .. })))
        && matches!(first, Ok(Finish::Exited(status)) if status.success())
}

#[cfg(test)]
mod tests;
