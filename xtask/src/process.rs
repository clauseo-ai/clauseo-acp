//! The only module that starts processes. Every other module goes through `capture`, `run_quiet`
//! or `run_inherit`, so the rules about how commands run live in one place.

use std::ffi::OsStr;
use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Size of each read from a running command's output pipe.
const CHUNK: usize = 8 * 1024;

/// The program a command runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Program {
    /// The cargo that started this xtask (the `CARGO` env var), falling back to `cargo`.
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

/// The variables `git rev-parse --local-env-vars` lists (git 2.54): each points git at a
/// particular repository, index or object store instead of the one found from the working dir.
const GIT_LOCAL_ENV: [&str; 15] = [
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
                Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
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

/// Runs `cmd` in `root` with its output going straight to the terminal. Returns whether it
/// succeeded.
pub(crate) fn run_inherit(cmd: &Cmd, root: &Path) -> io::Result<bool> {
    let status = cmd
        .program
        .command()
        .args(&cmd.args)
        .current_dir(root)
        .status()?;
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn git_runs_without_the_variables_that_point_it_at_another_repo() {
        let git = Program::Git.command();
        let removed: Vec<&OsStr> = git
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name)
            .collect();
        for name in GIT_LOCAL_ENV {
            assert!(removed.contains(&OsStr::new(name)), "{name} is removed");
        }
        assert_eq!(git.get_program(), "git");
    }

    #[test]
    fn cmd_displays_its_program_then_its_args() {
        let cmd = Cmd::new(Program::Cargo, ["fmt", "--all", "--", "--check"]);
        assert_eq!(cmd.to_string(), "cargo fmt --all -- --check");
        let named = Cmd::new(Program::Named("git"), Vec::<String>::new());
        assert_eq!(named.to_string(), "git");
        let pnpm = Cmd::new(Program::Pnpm, ["--version"]);
        assert_eq!(pnpm.to_string(), "pnpm --version");
    }

    /// The path of `name` in `dir`.
    fn at(dir: &str, name: &str) -> PathBuf {
        Path::new(dir).join(name)
    }

    /// A PATH value of `dirs`, joined the way the OS joins one.
    fn path_of(dirs: &[&str]) -> OsString {
        std::env::join_paths(dirs).expect("test directories hold no separator")
    }

    /// An `exists` predicate that holds for exactly `files`.
    fn only(files: &[PathBuf]) -> impl Fn(&Path) -> bool + '_ {
        move |file| files.iter().any(|held| held == file)
    }

    /// The directories of the PATH the tests search: the first holds no pnpm, the second one.
    const DIRS: [&str; 2] = ["/tools/bin", "/usr/local/node"];

    #[test]
    fn pnpm_resolves_to_a_cmd_file_on_path() {
        let files = [at("/usr/local/node", "pnpm.cmd")];
        assert_eq!(
            resolve_pnpm(&path_of(&DIRS), &WINDOWS_PNPM, only(&files)),
            at("/usr/local/node", "pnpm.cmd")
        );
    }

    #[test]
    fn pnpm_resolves_to_an_exe_when_there_is_no_cmd() {
        let files = [at("/tools/bin", "pnpm.exe")];
        assert_eq!(
            resolve_pnpm(&path_of(&DIRS), &WINDOWS_PNPM, only(&files)),
            at("/tools/bin", "pnpm.exe")
        );
    }

    #[test]
    fn pnpm_resolves_to_bare_pnpm_when_neither_is_on_path() {
        assert_eq!(
            resolve_pnpm(&path_of(&DIRS), &WINDOWS_PNPM, only(&[])),
            PathBuf::from("pnpm")
        );
    }

    #[test]
    fn an_exe_in_an_earlier_directory_beats_a_cmd_in_a_later_one() {
        let files = [
            at("/tools/bin", "pnpm.exe"),
            at("/usr/local/node", "pnpm.cmd"),
        ];
        assert_eq!(
            resolve_pnpm(&path_of(&DIRS), &WINDOWS_PNPM, only(&files)),
            at("/tools/bin", "pnpm.exe")
        );
    }

    #[test]
    fn an_exe_beats_a_cmd_in_the_same_directory() {
        let files = [
            at("/usr/local/node", "pnpm.cmd"),
            at("/usr/local/node", "pnpm.exe"),
        ];
        assert_eq!(
            resolve_pnpm(&path_of(&DIRS), &WINDOWS_PNPM, only(&files)),
            at("/usr/local/node", "pnpm.exe")
        );
    }

    #[test]
    fn the_resolved_path_is_the_directory_joined_with_the_name() {
        let files = [at("/usr/local/node", "pnpm.exe")];
        let resolved = resolve_pnpm(&path_of(&DIRS), &WINDOWS_PNPM, only(&files));
        assert_eq!(resolved.parent(), Some(Path::new("/usr/local/node")));
        assert_eq!(resolved.file_name(), Some(OsStr::new("pnpm.exe")));
    }

    #[cfg(unix)]
    #[test]
    fn capture_merges_stdout_and_stderr_and_reports_the_exit_status() {
        let cmd = Cmd::new(
            Program::Named("sh"),
            ["-c", "echo out; echo err >&2; exit 3"],
        );
        let mut seen = Vec::new();
        let captured = capture(&cmd, Path::new("."), &mut |chunk| {
            seen.extend_from_slice(chunk);
        })
        .expect("sh runs");
        assert!(!captured.success);
        assert!(captured.output.contains("out"));
        assert!(captured.output.contains("err"));
        assert_eq!(seen, captured.output.as_bytes());
    }
}
