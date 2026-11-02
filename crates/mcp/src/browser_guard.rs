//! The `browser-guard` mode: `clauseo-mcp browser-guard --agent-browser <file> --cwd <folder> --
//! <agent-browser args...>`.
//!
//! In order: read the command line (exit 2 on anything else), build the child's environment from
//! the host's variables (`env.rs`; exit 2 naming a missing or invalid one), on Unix make or
//! check the socket folder under `/tmp` (`socket.rs`; exit 2 if it is unsafe or too long), make
//! the folders under `--cwd` the variables point at, then start agent-browser and relay
//! (`relay.rs`), checking each agent line (`filter.rs`). `docs/decisions/0029-browser.md` gives
//! every rule its reason.

mod env;
mod filter;
mod relay;
#[cfg(any(unix, test))]
mod socket;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::{Failure, write};

/// The mode's command line.
pub const USAGE: &str = "usage: clauseo-mcp browser-guard --agent-browser <file> --cwd <folder> -- <agent-browser args...>";

/// The exit code for a bad command line or environment.
const EXIT_USAGE: i32 = 2;
/// The exit code when the guard cannot make its folders or start agent-browser.
const EXIT_FAILED: i32 = 1;

/// What the command line names.
#[derive(Debug, PartialEq, Eq)]
struct Command {
    agent_browser: PathBuf,
    cwd: PathBuf,
    child_args: Vec<OsString>,
}

/// Runs the guard with the arguments after `browser-guard`. Returns agent-browser's exit code
/// (1 when a signal killed it), or a [`Failure`].
pub fn run(args: &[OsString]) -> Result<i32, Failure> {
    let command = parse(args).map_err(|reason| Failure {
        code: EXIT_USAGE,
        message: format!("clauseo-mcp browser-guard: {reason}\n{USAGE}"),
    })?;
    let env = env::build(&env::Source::from_process(), env::Os::CURRENT, &command.cwd).map_err(
        |error| Failure {
            code: EXIT_USAGE,
            message: format!("clauseo-mcp browser-guard: {error}"),
        },
    )?;
    #[cfg(unix)]
    {
        let session = env
            .get(env::SESSION)
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let dir = env::socket_dir(env::Os::Unix, &command.cwd, session);
        socket::prepare(&dir, &command.cwd, session).map_err(|reason| Failure {
            code: EXIT_USAGE,
            message: format!("clauseo-mcp browser-guard: {reason}"),
        })?;
    }
    for folder in env::folders(env::Os::CURRENT, &command.cwd) {
        make_folder(&folder).map_err(|error| Failure {
            code: EXIT_FAILED,
            message: format!(
                "clauseo-mcp browser-guard: cannot make the folder {}: {error}",
                folder.display()
            ),
        })?;
    }
    let outcome = relay::run(
        &command.agent_browser,
        &command.child_args,
        &command.cwd,
        &env,
    )
    .map_err(|error| Failure {
        code: EXIT_FAILED,
        message: format!(
            "clauseo-mcp browser-guard: cannot run {}: {error}",
            command.agent_browser.display()
        ),
    })?;
    match outcome {
        relay::Outcome::Exited(code) => Ok(code),
        relay::Outcome::ChildLineTooLong => Err(Failure {
            code: EXIT_FAILED,
            message: format!(
                "clauseo-mcp browser-guard: agent-browser wrote a line over {} bytes; the guard \
                 killed it",
                relay::CHILD_MAX_LINE_BYTES
            ),
        }),
    }
}

/// Exactly `--agent-browser <file> --cwd <folder> -- <args...>`, in that order, each path
/// absolute and existing. The child's arguments are kept as given.
fn parse(args: &[OsString]) -> Result<Command, &'static str> {
    let [
        flag_program,
        program,
        flag_cwd,
        cwd,
        separator,
        child_args @ ..,
    ] = args
    else {
        return Err("expected --agent-browser <file> --cwd <folder> -- <args...>");
    };
    if flag_program != "--agent-browser" || flag_cwd != "--cwd" || separator != "--" {
        return Err("expected --agent-browser <file> --cwd <folder> -- <args...>");
    }
    let program = Path::new(program);
    if !program.is_absolute() || !program.is_file() {
        return Err("--agent-browser must be the absolute path of an existing file");
    }
    let cwd = Path::new(cwd);
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err("--cwd must be the absolute path of an existing folder");
    }
    Ok(Command {
        agent_browser: program.to_owned(),
        cwd: cwd.to_owned(),
        child_args: child_args.to_vec(),
    })
}

/// Makes `path` if it is missing, then insists it is a real folder, not a link or a file.
fn make_folder(path: &Path) -> io::Result<()> {
    match write::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    if std::fs::symlink_metadata(path)?.is_dir() {
        Ok(())
    } else {
        Err(io::Error::other("it exists and is not a folder"))
    }
}
