//! The `browser-guard` mode: `clauseo-mcp browser-guard --agent-browser <file> --cwd <folder>
//! --cdp-port <port> -- <agent-browser args...>`, with `--daemon-port <port>` after
//! `--cdp-port` on Windows.
//!
//! In order: note the guard's parent (`parent.rs`; exit 1 if it is gone already), read the
//! command line (exit 2 on anything else), build the child's environment from the host's
//! variables (`env.rs`; exit 2 naming a missing or invalid one), on Unix check the socket folder
//! the host made under `/tmp` (`socket.rs`; exit 2 if it is unsafe or too long), claim the run's
//! own names after sweeping the stale runs under `--cwd` (`runs.rs`: its profile, its lock file
//! and on Unix its agent-browser session name), make the folders under `--cwd` the variables
//! point at; on Unix start the reaper (`reaper.rs`), on Windows make the Job Object (`job.rs`);
//! start Chrome on the claimed debugging port
//! (`chrome.rs`; the reaper learns its pid, or the job holds it, before the guard waits for it;
//! exit 1 if it does not listen there within 15 s, or the parent goes meanwhile) with its
//! download mover (`downloads.rs`); then start agent-browser, connected to that Chrome, and
//! relay (`relay.rs`), checking each agent line (`filter.rs`) and, after each child line, that
//! the stream server and on Windows the daemon are on their claimed ports (`port_file.rs`),
//! until agent-browser exits, Chrome exits or the guard's parent goes (`parent.rs`). On the way
//! out, whatever the reason, the guard ends agent-browser's daemon and Chrome and deletes
//! the run's profile and lock file. `docs/decisions/0029-browser.md` gives every rule its reason.

mod chrome;
mod downloads;
mod env;
mod filter;
#[cfg(windows)]
mod job;
mod parent;
mod port_file;
mod quiet;
#[cfg(unix)]
pub mod reaper;
mod relay;
mod runs;
#[cfg(any(unix, test))]
mod socket;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    /// Chrome's debugging port, passed to the guard's Chrome as `--remote-debugging-port`.
    cdp_port: u16,
    /// Windows only: the port agent-browser's daemon must listen on.
    daemon_port: Option<u16>,
    child_args: Vec<OsString>,
}

/// Runs the guard with the arguments after `browser-guard`. Returns agent-browser's exit code
/// (1 when a signal killed it), or a [`Failure`].
pub fn run(args: &[OsString]) -> Result<i32, Failure> {
    // First, before anything slow: a parent that dies later is then still the one watched.
    let parent = parent::Parent::now().ok_or_else(|| failed(PARENT_GONE))?;
    let command = parse(args, env::Os::CURRENT).map_err(|reason| Failure {
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
        let dir = env.get(env::SOCKET_DIR).map_or(Path::new(""), Path::new);
        socket::check(dir, &command.cwd, &runs::session_of(runs::LONGEST_ID)).map_err(
            |reason| Failure {
                code: EXIT_USAGE,
                message: format!("clauseo-mcp browser-guard: {reason}"),
            },
        )?;
    }
    let run = runs::Run::claim(&command.cwd)
        .map_err(|error| failed(&format!("cannot claim a run under --cwd: {error}")))?;
    // Unix: the run's own session name, so its daemon's socket is the run's own too.
    #[cfg(unix)]
    let (env, session) = {
        let mut env = env;
        let session = runs::session_of(run.id());
        env.insert(env::SESSION.to_owned(), OsString::from(&session));
        (env, session)
    };
    // Windows: the host's, from which agent-browser derives the daemon port the host claimed.
    #[cfg(windows)]
    let session = env
        .get(env::SESSION)
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned();
    for folder in env::folders(env::Os::CURRENT, &command.cwd) {
        make_folder(&folder).map_err(|error| {
            failed(&format!(
                "cannot make the folder {}: {error}",
                folder.display()
            ))
        })?;
    }
    serve(&command, env, &session, &parent, &run)
}

/// Starts Chrome and agent-browser and relays; ends both, whatever happens, before returning.
fn serve(
    command: &Command,
    mut env: BTreeMap<String, OsString>,
    session: &str,
    parent: &parent::Parent,
    run: &runs::Run,
) -> Result<i32, Failure> {
    let socket_dir = env
        .get(env::SOCKET_DIR)
        .map_or_else(|| env::windows_socket_dir(&command.cwd), PathBuf::from);
    #[cfg(unix)]
    let reaper = reaper::Reaper::start(&socket_dir, session, &command.cwd, run.id())
        .map_err(|error| failed(&format!("cannot start the reaper: {error}")))?;
    #[cfg(windows)]
    let job = job::Job::new().map_err(|error| failed(&error.to_string()))?;
    let chrome_env = env::chrome_env(&env);
    let spec = chrome::Spec {
        executable: env
            .get(env::EXECUTABLE_PATH)
            .map_or(Path::new(""), Path::new),
        cwd: &command.cwd,
        profile: run.profile(),
        cdp_port: command.cdp_port,
        proxy: env
            .get(env::PROXY)
            .map_or(OsStr::new(""), OsString::as_os_str),
        env: &chrome_env,
    };
    let launched = chrome::launch(
        &spec,
        #[cfg(windows)]
        &job,
    );
    let mut chrome = match launched {
        Ok(chrome) => chrome,
        Err(reason) => {
            #[cfg(unix)]
            reaper.done();
            return Err(failed(&reason));
        }
    };
    // Before the wait for Chrome's endpoint: a guard killed during it leaves the reaper to end
    // Chrome's group. Once the guard reaps Chrome, the reaper leaves its pid alone.
    #[cfg(unix)]
    {
        reaper.chrome(chrome.pid());
        let teller = reaper.teller();
        chrome.on_reap(move || teller.say("reaped chrome"));
    }
    note(&format!(
        "Chrome started; waiting for it to listen on the claimed debugging port {}",
        command.cdp_port
    ));
    let endpoint = match chrome.endpoint(command.cdp_port, parent) {
        Ok(endpoint) => endpoint,
        Err(reason) => {
            drop(chrome);
            #[cfg(unix)]
            reaper.done();
            return Err(failed(&reason));
        }
    };
    note(&format!(
        "Chrome is listening on the claimed debugging port {}",
        command.cdp_port
    ));
    env::set_cdp(&mut env, &endpoint);
    let agent = relay::Agent {
        program: &command.agent_browser,
        args: &command.child_args,
        cwd: &command.cwd,
        env: &env,
        port_checks: port_checks(&env, command, session),
        #[cfg(unix)]
        on_spawn: &mut |pid| reaper.agent(pid),
        #[cfg(windows)]
        on_spawn: &mut |_| {},
        #[cfg(unix)]
        on_reap: &mut || reaper.reaped_agent(),
        #[cfg(windows)]
        on_reap: &mut || {},
    };
    let outcome = relay::run(
        agent,
        &mut chrome,
        parent,
        #[cfg(windows)]
        &job,
    );
    #[cfg(unix)]
    reaper::kill_daemon(&socket_dir, session);
    // Windows: the job first, so Chrome's helpers are gone before its profile is deleted.
    #[cfg(windows)]
    if !job.kill() {
        note("Windows refused to end the guard's job; Chrome's drop kills Chrome alone");
    }
    // Kills Chrome's tree, moves the last downloads and deletes the profile.
    drop(chrome);
    #[cfg(windows)]
    drop((job, socket_dir));
    #[cfg(unix)]
    reaper.done();
    let outcome = outcome.map_err(|error| {
        failed(&format!(
            "cannot run {}: {error}",
            command.agent_browser.display()
        ))
    })?;
    outcome_code(outcome)
}

/// agent-browser's exit code, or the failure for a relay the guard ended.
fn outcome_code(outcome: relay::Outcome) -> Result<i32, Failure> {
    match outcome {
        relay::Outcome::Exited(code) => Ok(code),
        relay::Outcome::ChildLineTooLong => Err(failed(&format!(
            "agent-browser wrote a line over {} bytes; the guard killed it",
            relay::CHILD_MAX_LINE_BYTES
        ))),
        relay::Outcome::PortRefused(reason) => {
            Err(failed(&format!("{reason}; the guard ended agent-browser")))
        }
        relay::Outcome::ChromeExited => Err(failed("Chrome exited; the guard ended agent-browser")),
        relay::Outcome::ParentGone => Err(failed(PARENT_GONE)),
    }
}

/// The failure when the guard's parent has gone (`parent.rs`).
pub(crate) const PARENT_GONE: &str =
    "the process that started the guard has gone; the guard ended the session's browser";

/// An exit-1 failure with the guard's prefix.
fn failed(reason: &str) -> Failure {
    Failure {
        code: EXIT_FAILED,
        message: format!("clauseo-mcp browser-guard: {reason}"),
    }
}

/// One line on the guard's stderr. Diagnostics only; a failed write has nowhere to go.
pub(crate) fn note(text: &str) {
    let mut stderr = io::stderr().lock();
    if writeln!(stderr, "clauseo-mcp browser-guard: {text}").is_err() {}
}

/// Blocks the calling thread for `time`: the guard's polls (Chrome's exit, the parent, the
/// downloads, the profile's deletion) run on plain threads, with no clock to inject.
#[expect(
    clippy::disallowed_methods,
    reason = "the guard's own polling threads; no core logic depends on the time"
)]
pub(crate) fn pause(time: Duration) {
    std::thread::sleep(time);
}

/// The port files the relay checks after each child line: the stream server's in the socket
/// folder (the host's on Unix, `<cwd>\run` on Windows), and the daemon's on Windows.
fn port_checks(
    env: &BTreeMap<String, OsString>,
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
