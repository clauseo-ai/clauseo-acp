//! The `browser-guard` mode: `clauseo-mcp browser-guard --agent-browser <file> --cwd <folder>
//! --cdp-port <port> -- <agent-browser args...>`, with `--daemon-port <port>` after
//! `--cdp-port` on Windows.
//!
//! In order: note the guard's parent (`parent.rs`; exit 1 if it is gone already), read the
//! command line (exit 2 on anything else), build the child's environment from the host's
//! variables (`env.rs`; exit 2 naming a missing or invalid one), on Unix check the socket folder
//! the host made under `/tmp` (`socket.rs`; exit 2 if it is unsafe or too long), claim the run's
//! own names after sweeping the stale runs under `--cwd` (`runs.rs`: its profile, its lock file,
//! its Chrome record (`record.rs`) and its agent-browser session name), make the folders under
//! `--cwd` the variables point at; on Unix start the reaper (`reaper.rs`), on Windows make the
//! Job Object (`job.rs`); then relay (`relay.rs`), checking each agent line (`filter.rs`).
//!
//! Chrome starts lazily (`session.rs`): while it is stopped, a request that is no tool call
//! goes to a one-shot agent-browser with no Chrome (`relay/oneshot.rs`). The first tool call
//! starts Chrome on the claimed debugging port (`chrome.rs`; the reaper learns its pid, or the
//! job holds it, before the guard waits for it) with its download mover (`downloads.rs`), then
//! agent-browser connected to it; after each child line the guard checks that the stream server
//! and on Windows the daemon are on their claimed ports (`port_file.rs`). Ten minutes without a
//! tool call stop Chrome, agent-browser and its daemon, keeping the profile; Chrome's first
//! unexpected exit stops them too, and its second ends the guard. The guard ends when the
//! agent's stdin ends, agent-browser exits, a start fails or the guard's parent goes
//! (`parent.rs`). On the way out, whatever the reason, it ends agent-browser's daemon and Chrome
//! and deletes the run's profile, its Chrome record and its lock file. [`runs_settled`] and
//! [`chrome_groups`] let the host read the runs under a `--cwd`.
//! `docs/decisions/0029-browser.md` gives every rule its reason.

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
mod record;
mod relay;
mod runs;
mod session;
#[cfg(any(unix, test))]
mod socket;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::{Failure, write};
pub use record::{ChromeProcess, ChromeState};

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
    record::write_state(&command.cwd, run.id(), &record::ChromeState::None);
    // Unix: the run's own session name, so its daemon's socket is the run's own too.
    #[cfg(unix)]
    let (env, session) = {
        let mut env = env;
        let session = runs::session_of(run.id());
        env.insert(env::SESSION.to_owned(), OsString::from(&session));
        (env, session)
    };
    // Windows: the run's own too, one whose derived daemon port is the one the host claimed.
    #[cfg(windows)]
    let (env, session) = {
        let mut env = env;
        let host = env
            .get(env::SESSION)
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_owned();
        let own = command
            .daemon_port
            .and_then(|port| runs::windows_session(run.id(), port));
        let session = own.unwrap_or_else(|| {
            note("no run name fits the claimed daemon port; using the host's session name");
            host
        });
        env.insert(env::SESSION.to_owned(), OsString::from(&session));
        (env, session)
    };
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

/// Relays, starting Chrome and agent-browser at the first tool call (`session.rs`); ends both,
/// whatever happens, and deletes the run's profile before returning.
fn serve(
    command: &Command,
    env: BTreeMap<String, OsString>,
    session: &str,
    parent: &parent::Parent,
    run: &runs::Run,
) -> Result<i32, Failure> {
    #[cfg(unix)]
    let socket_dir = env
        .get(env::SOCKET_DIR)
        .map_or_else(PathBuf::new, PathBuf::from);
    #[cfg(unix)]
    let reaper = reaper::Reaper::start(&socket_dir, session, &command.cwd, run.id())
        .map_err(|error| failed(&format!("cannot start the reaper: {error}")))?;
    let launcher = session::Launcher {
        executable: env
            .get(env::EXECUTABLE_PATH)
            .map_or_else(PathBuf::new, PathBuf::from),
        cwd: command.cwd.clone(),
        run: run.id().to_owned(),
        profile: run.profile().to_owned(),
        cdp_port: command.cdp_port,
        proxy: env.get(env::PROXY).cloned().unwrap_or_default(),
        chrome_env: env::chrome_env(&env),
        program: command.agent_browser.clone(),
        args: command.child_args.clone(),
        port_checks: port_checks(&env, command, session),
        env,
        #[cfg(unix)]
        socket_dir,
        #[cfg(unix)]
        session: session.to_owned(),
        idle: session::idle_stop(),
        #[cfg(unix)]
        reaper: reaper.teller(),
        #[cfg(windows)]
        job: job::Job::new().map_err(|error| failed(&error.to_string()))?,
    };
    let shared = Arc::new(session::Shared::new(launcher));
    // Stops whatever runs (Chrome's tree, agent-browser's, the daemon) before it returns.
    let outcome = relay::run(&shared, parent);
    chrome::remove_profile(run.profile());
    // Chrome is reaped: the record goes, before the run's lock (`runs::Run`'s drop).
    record::clear(&command.cwd, run.id());
    #[cfg(unix)]
    reaper.done();
    outcome_code(outcome, &command.agent_browser)
}

/// agent-browser's exit code, or the failure for a relay the guard ended.
fn outcome_code(outcome: relay::Outcome, agent_browser: &Path) -> Result<i32, Failure> {
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
        relay::Outcome::StartFailed(reason) => Err(failed(&reason)),
        relay::Outcome::AgentFailed(error) => Err(failed(&format!(
            "cannot run {}: {error}",
            agent_browser.display()
        ))),
    }
}

/// Whether every guard run under `cwd` (the guard's `--cwd`) has finished its cleanup: no run's
/// profile folder is left and no run's lock file is held by its guard or reaper. It means
/// "cleanup finished", not "Chrome dead": Chrome can outlive a SIGKILL of both the guard and
/// the reaper, and a run whose browser idled out stays unsettled until its guard exits.
/// It takes no lock a live run would notice (`runs.rs`; decision record 0029, "Settled runs").
pub fn runs_settled(cwd: &Path) -> bool {
    runs::settled(cwd)
}

/// The Chrome of each guard run under `cwd` that is not settled ([`runs_settled`]), and of
/// each run whose Chrome record was left naming one: `None` (no Chrome now), `Starting` (about
/// to spawn, or spawned and not yet recorded, or not to be told: count it as alive) or
/// `Running` with the process to probe ([`ChromeProcess::gone`]). Read-only; never "gone" while
/// Chrome may live (decision record 0029, "Chrome records").
pub fn chrome_groups(cwd: &Path) -> Vec<ChromeState> {
    record::chrome_groups(cwd)
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

/// The time now, for the guard's idle stop: the guard runs on plain threads, with no clock to
/// inject.
#[expect(
    clippy::disallowed_methods,
    reason = "the guard's own idle timer; no core logic depends on the time"
)]
pub(crate) fn now() -> std::time::Instant {
    std::time::Instant::now()
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
