//! The `browser-guard` mode: `clauseo-mcp browser-guard --agent-browser <file> --cwd <folder>
//! --cdp-port <port> -- <agent-browser args...>`, with `--daemon-port <port>` after
//! `--cdp-port` on Windows.
//!
//! In order: read the command line (exit 2 on anything else), build the child's environment from
//! the host's variables (`env.rs`; exit 2 naming a missing or invalid one), on Unix check the
//! socket folder the host made under `/tmp` (`socket.rs`; exit 2 if it is unsafe or too long),
//! make the folders under `--cwd` the variables point at, then start agent-browser and relay
//! (`relay.rs`), checking each agent line (`filter.rs`) and, after each child line, that the
//! stream server and on Windows the daemon are on their claimed ports (`port_file.rs`); on
//! Windows inside a Job Object (`job.rs`). `docs/decisions/0029-browser.md` gives every rule its
//! reason.

mod env;
mod filter;
#[cfg(windows)]
mod job;
mod port_file;
mod relay;
#[cfg(any(unix, test))]
mod socket;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::{Failure, write};

/// The mode's command line on this OS.
pub const USAGE: &str = if cfg!(windows) {
    USAGE_WINDOWS
} else {
    USAGE_UNIX
};
/// The command line on macOS and Linux.
const USAGE_UNIX: &str = "usage: clauseo-mcp browser-guard --agent-browser <file> --cwd <folder> --cdp-port <port> -- <agent-browser args...>";
/// The command line on Windows: the daemon's port too, which agent-browser derives from the
/// session name and the host claimed (`port_file.rs`).
const USAGE_WINDOWS: &str = "usage: clauseo-mcp browser-guard --agent-browser <file> --cwd <folder> --cdp-port <port> --daemon-port <port> -- <agent-browser args...>";

/// The exit code for a bad command line or environment.
const EXIT_USAGE: i32 = 2;
/// The exit code when the guard cannot make its folders or start agent-browser.
const EXIT_FAILED: i32 = 1;

/// What the command line names.
#[derive(Debug, PartialEq, Eq)]
struct Command {
    agent_browser: PathBuf,
    cwd: PathBuf,
    /// Chrome's debugging port, passed as `--remote-debugging-port`.
    cdp_port: u16,
    /// Windows only: the port agent-browser's daemon must listen on.
    daemon_port: Option<u16>,
    child_args: Vec<OsString>,
}

/// Runs the guard with the arguments after `browser-guard`. Returns agent-browser's exit code
/// (1 when a signal killed it), or a [`Failure`].
pub fn run(args: &[OsString]) -> Result<i32, Failure> {
    let command = parse(args, env::Os::CURRENT).map_err(|reason| Failure {
        code: EXIT_USAGE,
        message: format!("clauseo-mcp browser-guard: {reason}\n{USAGE}"),
    })?;
    let env = env::build(
        &env::Source::from_process(),
        env::Os::CURRENT,
        &command.cwd,
        command.cdp_port,
    )
    .map_err(|error| Failure {
        code: EXIT_USAGE,
        message: format!("clauseo-mcp browser-guard: {error}"),
    })?;
    let session = env
        .get(env::SESSION)
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned();
    #[cfg(unix)]
    {
        let dir = env.get(env::SOCKET_DIR).map_or(Path::new(""), Path::new);
        socket::check(dir, &command.cwd, &session).map_err(|reason| Failure {
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
    let port_checks = port_checks(&env, &command, &session);
    let outcome = relay::run(
        &command.agent_browser,
        &command.child_args,
        &command.cwd,
        &env,
        port_checks,
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
        relay::Outcome::PortRefused(reason) => Err(Failure {
            code: EXIT_FAILED,
            message: format!("clauseo-mcp browser-guard: {reason}; the guard ended agent-browser"),
        }),
    }
}

/// The port files the relay checks after each child line: the stream server's in the socket
/// folder (the host's on Unix, `<cwd>\run` on Windows), and the daemon's on Windows.
fn port_checks(
    env: &std::collections::BTreeMap<String, OsString>,
    command: &Command,
    session: &str,
) -> Vec<port_file::Expected> {
    let socket_dir = env
        .get(env::SOCKET_DIR)
        .map_or_else(|| env::windows_socket_dir(&command.cwd), PathBuf::from);
    let mut checks = Vec::new();
    // `env::build` checked the stream port already.
    if let Some(port) = env
        .get(env::STREAM_PORT)
        .and_then(|value| env::port_of(value))
    {
        checks.push(port_file::Expected::stream(&socket_dir, session, port));
    }
    if let Some(port) = command.daemon_port {
        checks.push(port_file::Expected::daemon(&socket_dir, session, port));
    }
    checks
}

/// What the command line must look like, for its refusals.
const EXPECTED_UNIX: &str =
    "expected --agent-browser <file> --cwd <folder> --cdp-port <port> -- <args...>";
const EXPECTED_WINDOWS: &str = "expected --agent-browser <file> --cwd <folder> --cdp-port <port> --daemon-port <port> -- <args...>";

/// Exactly `--agent-browser <file> --cwd <folder> --cdp-port <port> -- <args...>`, in that
/// order, with `--daemon-port <port>` after `--cdp-port` on Windows and never on Unix; each
/// path absolute and existing, each port from 1024 to 65535 in plain decimal. The child's
/// arguments are kept as given.
fn parse(args: &[OsString], os: env::Os) -> Result<Command, &'static str> {
    let expected = match os {
        env::Os::Unix => EXPECTED_UNIX,
        env::Os::Windows => EXPECTED_WINDOWS,
    };
    let [
        flag_program,
        program,
        flag_cwd,
        cwd,
        flag_cdp,
        cdp,
        rest @ ..,
    ] = args
    else {
        return Err(expected);
    };
    if flag_program != "--agent-browser" || flag_cwd != "--cwd" || flag_cdp != "--cdp-port" {
        return Err(expected);
    }
    let (daemon, separator, child_args) = match (os, rest) {
        (env::Os::Windows, [flag_daemon, daemon, separator, child_args @ ..])
            if flag_daemon == "--daemon-port" =>
        {
            (Some(daemon), separator, child_args)
        }
        (env::Os::Unix, [flag_daemon, ..]) if flag_daemon == "--daemon-port" => {
            return Err("--daemon-port is for Windows only");
        }
        (env::Os::Unix, [separator, child_args @ ..]) => (None, separator, child_args),
        _ => return Err(expected),
    };
    if separator != "--" {
        return Err(expected);
    }
    let program = Path::new(program);
    if !program.is_absolute() || !program.is_file() {
        return Err("--agent-browser must be the absolute path of an existing file");
    }
    let cwd = Path::new(cwd);
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err("--cwd must be the absolute path of an existing folder");
    }
    let cdp_port = env::port_of(cdp)
        .ok_or("--cdp-port must be a port from 1024 to 65535 with no leading zero")?;
    let daemon_port = match daemon {
        Some(daemon) => Some(
            env::port_of(daemon)
                .ok_or("--daemon-port must be a port from 1024 to 65535 with no leading zero")?,
        ),
        None => None,
    };
    Ok(Command {
        agent_browser: program.to_owned(),
        cwd: cwd.to_owned(),
        cdp_port,
        daemon_port,
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

#[cfg(test)]
mod tests;
