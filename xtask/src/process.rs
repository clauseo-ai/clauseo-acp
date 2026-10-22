//! The only module that starts processes. Every other module goes through `capture`, `run_quiet`
//! or `run`, and those three through `running.rs` and the one `spawn` here, so the rules about how
//! commands run live in one place:
//! - A command cannot run without a time limit: `Cmd::new` takes it.
//! - It runs with stdin closed, in a process tree of its own: a new process group on Unix, a Job
//!   Object that kills its processes when its handle closes on Windows (`windows.rs`).
//! - Its tree is stopped at the limit and when its run is dropped (a panic included): on Unix
//!   SIGTERM to the group, then SIGKILL after `KILL_GRACE`; on Windows `TerminateJobObject`, which
//!   ends every process in the job at once. What is left of the tree when the command exits gets
//!   the same stop on Unix (SIGTERM, then SIGKILL if the group has not emptied after
//!   `KILL_GRACE`), and is ended with the job on Windows (`running.rs`).
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
mod test_mode;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use leader::lead;
pub(crate) use test_mode::{TestOutput, test_mode};

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
/// directory is on PATH. A second lookup reuses it instead of splitting PATH again. An empty
/// entry (`;;` on Windows, `::` elsewhere) is skipped: joined to a file name it would name a
/// file in the working directory, so a `pnpm.exe` wherever xtask runs would win.
#[expect(
    clippy::disallowed_methods,
    reason = "the one walk over PATH; clippy.toml bans split_paths everywhere else"
)]
pub(crate) fn first_on_path<T>(
    path: &OsStr,
    mut found: impl FnMut(&Path) -> Option<T>,
) -> Option<T> {
    std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .find_map(|dir| found(&dir))
}

/// A program, its arguments and how long it may run. Displays as the shell-like form:
/// `cargo fmt --all -- --check`, after its `NAME=value` variables when it sets any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cmd {
    pub program: Program,
    pub args: Vec<String>,
    /// How long it may run before its tree is stopped. Every call site passes a named `const`
    /// that says why that long.
    pub timeout: Duration,
    /// How long its tree has between SIGTERM and SIGKILL on Unix, at its limit and for what is
    /// left of it when it exits: `KILL_GRACE` unless `kill_grace` set another.
    pub kill_grace: Duration,
    /// Variables set for it on top of the environment it inherits from xtask, in order.
    pub env: Vec<(String, String)>,
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
            kill_grace: running::KILL_GRACE,
            env: Vec::new(),
        }
    }

    /// `self` with `name` set to `value` in its environment. A test uses it to give a cargo run
    /// the setting a real run gets from its job, whatever the test's own job sets.
    #[cfg(test)]
    pub(crate) fn env(mut self, name: &str, value: &str) -> Cmd {
        self.env.push((name.to_owned(), value.to_owned()));
        self
    }

    /// `self` with `grace` between SIGTERM and SIGKILL instead of `KILL_GRACE`, for a command
    /// that needs longer to stop its own children. Pass a named `const` that says why.
    pub(crate) fn kill_grace(self, grace: Duration) -> Cmd {
        Cmd {
            kill_grace: grace,
            ..self
        }
    }
}

impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (name, value) in &self.env {
            write!(f, "{name}={value} ")?;
        }
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

/// How a command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    /// It exited with status 0.
    Succeeded,
    /// It exited with another status, or a signal ended it.
    Failed,
    /// It was still running at its limit, so its tree was stopped. Its message is
    /// `timeout_message` and its fix `TimeoutFix`.
    TimedOut,
}

/// How a command run by `capture`, `run_quiet` or `run` ended, and what was kept of its output:
/// the one result of every run. A timeout is a `Status`, never an error: the `Err` beside a
/// `Ran` means only that the command could not be started, or that its exit or output could not
/// be waited for or read (a process that left its tree holding the output included).
#[derive(Debug)]
pub(crate) struct Ran {
    pub status: Status,
    /// Its stdout; from `capture`, its stdout and stderr merged in the order they came. Empty with
    /// `Output::Inherit`, whose output went to xtask's own and was not kept. What came before the
    /// stop when it timed out.
    pub stdout: String,
    /// Its stderr. Empty from `capture`, which merges it into `stdout`, and with
    /// `Output::Inherit`.
    pub stderr: String,
}

impl Ran {
    /// Whether it exited with status 0.
    pub(crate) fn success(&self) -> bool {
        self.status == Status::Succeeded
    }

    /// The result of a run that ended as `finish` says, with `stdout` and `stderr` kept.
    fn of(finish: &Finish, stdout: &[u8], stderr: &[u8]) -> Ran {
        let status = match finish {
            Finish::Exited(status) if status.success() => Status::Succeeded,
            Finish::Exited(_) => Status::Failed,
            Finish::TimedOut => Status::TimedOut,
        };
        Ran {
            status,
            stdout: String::from_utf8_lossy(stdout).into_owned(),
            stderr: String::from_utf8_lossy(stderr).into_owned(),
        }
    }
}

/// The results a test's fake runner returns.
#[cfg(test)]
impl Ran {
    /// A run that exited, with `success` saying whether with status 0.
    pub(crate) fn exited(
        success: bool,
        stdout: impl Into<String>,
        stderr: impl Into<String>,
    ) -> Ran {
        Ran {
            status: if success {
                Status::Succeeded
            } else {
                Status::Failed
            },
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    /// A run stopped at its limit, with nothing kept.
    pub(crate) fn timed_out() -> Ran {
        Ran {
            status: Status::TimedOut,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

/// Runs `cmd` in `root` with stdout and stderr merged into one stream. Each chunk read is passed
/// to `on_output` as it arrives, and kept for the returned `stdout`. A command that exits while a
/// process that left its tree still holds its output is an error that says so, after
/// `running::DRAIN`.
pub(crate) fn capture(cmd: &Cmd, root: &Path, on_output: &mut dyn FnMut(&[u8])) -> io::Result<Ran> {
    let mut bytes = Vec::new();
    let finish = running::run(cmd, root, Streams::Merged, &mut |_, chunk| {
        bytes.extend_from_slice(chunk);
        on_output(chunk);
    })?;
    Ok(Ran::of(&finish, &bytes, b""))
}

/// A runner of `run_quiet`: production passes `run_quiet` itself, a test a fake.
pub(crate) type QuietRunner<'a> = dyn FnMut(&Cmd, &Path) -> io::Result<Ran> + 'a;

/// Runs `cmd` in `root` and keeps its stdout and stderr apart: `run` with `Output::Quiet`.
pub(crate) fn run_quiet(cmd: &Cmd, root: &Path) -> io::Result<Ran> {
    run(cmd, root, Output::Quiet)
}

/// Where the output of a command run by `run` goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Output {
    /// Straight to xtask's stdout and stderr, so the terminal when xtask has one. On Unix the
    /// command still runs in a process group of its own: Ctrl-C reaches only xtask, and the leader
    /// stops the command when xtask dies of it; it cannot read the terminal, so git and ssh
    /// prompts are turned off (`unix.rs`). On Windows it runs in its own job and shares xtask's
    /// console, so Ctrl-C reaches it as well as xtask, and Windows ends the job when xtask dies.
    Inherit,
    /// Kept, stdout and stderr apart.
    Quiet,
}

/// Runs `cmd` in `root` with its output going where `output` says, and stops its tree if it runs
/// past its limit. With `Output::Quiet`, output a process that left its tree still holds is an
/// error, as for `capture`.
pub(crate) fn run(cmd: &Cmd, root: &Path, output: Output) -> io::Result<Ran> {
    let streams = match output {
        Output::Inherit => Streams::Inherit,
        Output::Quiet => Streams::Separate,
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let finish = running::run(cmd, root, streams, &mut |stream, chunk| match stream {
        Stream::Out => stdout.extend_from_slice(chunk),
        Stream::Err => stderr.extend_from_slice(chunk),
    })?;
    Ok(Ran::of(&finish, &stdout, &stderr))
}

/// What to say of `cmd` when it ran past its limit (`Status::TimedOut`). The one home of that
/// wording; its fix is `TimeoutFix`.
pub(crate) fn timeout_message(cmd: &Cmd) -> String {
    format!(
        "`{cmd}` did not finish within {}, so it was stopped",
        limit_text(cmd.timeout)
    )
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

#[cfg(test)]
mod tests;
