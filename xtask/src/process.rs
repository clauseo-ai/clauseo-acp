//! The only module that starts processes. Every other module goes through `capture`, `run_quiet`
//! or `run_inherit`, so the rules about how commands run live in one place.

use std::fmt;
use std::io::{self, Read};
use std::path::Path;
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
}

impl Program {
    /// The name people see: `cargo`, or the named program.
    pub(crate) fn name(&self) -> &'static str {
        match *self {
            Program::Cargo => "cargo",
            Program::Named(name) => name,
        }
    }

    fn command(&self) -> Command {
        match *self {
            Program::Cargo => {
                Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            }
            Program::Named(name) => Command::new(name),
        }
    }
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

/// Runs `cmd` in `root` with its output going straight to the terminal. Returns whether it succeeded.
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

    #[test]
    fn cmd_displays_its_program_then_its_args() {
        let cmd = Cmd::new(Program::Cargo, ["fmt", "--all", "--", "--check"]);
        assert_eq!(cmd.to_string(), "cargo fmt --all -- --check");
        let named = Cmd::new(Program::Named("git"), Vec::<String>::new());
        assert_eq!(named.to_string(), "git");
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
