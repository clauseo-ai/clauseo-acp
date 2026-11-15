//! The `browser-guard` mode: `clauseo-mcp browser-guard --agent-browser <file> --cwd <folder>
//! [--downloads <folder> [--fallback-downloads <folder>]] --cdp-port <port> -- <agent-browser
//! args...>`, with `--daemon-port <port>` after `--cdp-port` on Windows. `--downloads` is where
//! Chrome's downloads are delivered (`downloads.rs`): `<matter>/downloads` in a Codex session,
//! made here if missing; without it, `<cwd>/downloads`. `--fallback-downloads` is where a
//! download goes when that folder's drive cannot keep the "from the internet" mark
//! (`<data>/agent-state/<matter>/downloads`, made when first needed: `downloads/deliver.rs`).
//!
//! In order: note the guard's parent (`parent.rs`; exit 1 if it is gone already), read the command
//! line (exit 2 on anything else), build the child's environment from the host's variables and read
//! the proxy's password from the file `CLAUSEO_PROXY_SECRET_FILE` names, deleting the file, which
//! no child sees (`env.rs`; exit 2 naming a missing or invalid one), on Unix check the socket
//! folder the host made under `/tmp` (`socket.rs`; exit 2 if it is unsafe or too long), claim
//! the run's own names after sweeping the stale runs under `--cwd` (`runs.rs`: its profile, its
//! lock file, its Chrome record (`record.rs`) and its agent-browser session name), make the
//! folders under `--cwd` the variables point at; on Unix start the reaper (`reaper.rs`), on
//! Windows make the Job Object (`job.rs`); then relay (`relay.rs`), checking each agent line
//! (`filter.rs`).
//!
//! Chrome starts lazily (`session.rs`): while it is stopped, a request that is no tool call
//! goes to a one-shot agent-browser with no Chrome (`relay/oneshot.rs`). The first tool call
//! starts the browser over its `DevTools` pipe (`chrome.rs`; the reaper learns its pid, or the
//! job holds it, before the guard waits for it) with its download mover (`downloads.rs`), has
//! the guard's own client on that pipe (`devtools/seam.rs`) answer the session proxy's challenge
//! with the password (`proxy_auth.rs`; the host gives every entry, the plain client's included,
//! the proxy and its password file, decision record 0053), then agent-browser connected to the
//! relay; on Windows, after each child line, the guard checks that the stream server is off and
//! the daemon is on its claimed port (`port_file.rs`). Ten minutes without a tool call stop
//! Chrome, agent-browser and its daemon, keeping the profile; Chrome's first unexpected exit stops
//! them too, and its second ends the guard. The guard ends when the agent's stdin ends,
//! agent-browser exits, a start fails or the guard's parent goes (`parent.rs`). On the way out,
//! whatever the reason, it ends agent-browser's daemon and Chrome and deletes the run's profile,
//! its Chrome record and its lock file. [`runs_settled`] and [`chrome_groups`] let the host read
//! the runs under a `--cwd`. `docs/decisions/0029-browser.md` gives every rule its reason.
//!
//! Route 2 (`docs/decisions/0060-browser-devtools-pipe.md`), on every system: the browser has
//! no debugging port. The guard starts it over a `DevTools` pipe (`chrome/pipe.rs`) and relays the
//! pipe to agent-browser on a 127.0.0.1 websocket with a 256-bit secret path, letting in only
//! agent-browser's own processes (`devtools.rs`, `devtools/peer.rs`); `--cdp-port` is unused,
//! and agent-browser is told to start no stream server (`env.rs`, `port_file.rs`). The guard
//! first hides itself from other processes of the user (`harden.rs`; exit 1 if it cannot): on
//! Windows a protected DACL from its creation, by running as a copy of itself that its first
//! process starts hardened (passing on its exit code), which every child it starts gets too; on
//! Linux `PR_SET_DUMPABLE` 0, its first act. On Windows the browser is Edge, or else a Program
//! Files Chrome (`browsers.rs`).

#[cfg(any(windows, test))]
mod browsers;
mod chrome;
#[cfg(any(windows, test))]
mod command_line;
mod devtools;
mod downloads;
mod env;
mod filter;
mod harden;
#[cfg(windows)]
mod job;
mod logon;
mod parent;
mod port_file;
mod proxy_auth;
mod quiet;
#[cfg(unix)]
pub mod reaper;
mod record;
mod relay;
mod runs;
mod session;
#[cfg(any(unix, test))]
mod socket;

/// The guard's children and their pipes: std's on Unix; on Windows the job's own, created in
/// the guard's job (`job/spawn.rs`), which std's `Command` cannot do.
#[cfg(windows)]
use job::{Child, ChildStderr, ChildStdin};
#[cfg(unix)]
use std::process::{Child, ChildStderr, ChildStdin};

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
/// agent-browser's download folder variable, set to `--downloads`.
const DOWNLOAD_PATH: &str = "AGENT_BROWSER_DOWNLOAD_PATH";
/// How long the guard waits on its way out for agent-browser's last stderr (`relay/redact.rs`).
const STDERR_DRAIN: Duration = Duration::from_secs(2);
/// The exit code when the guard cannot make its folders or start agent-browser.
const EXIT_FAILED: i32 = 1;

/// What the command line names.
#[derive(Debug, PartialEq, Eq)]
struct Command {
    agent_browser: PathBuf,
    cwd: PathBuf,
    /// `--downloads`, or `<cwd>/downloads`.
    downloads: PathBuf,
    /// `--fallback-downloads`: where a download goes when the downloads folder's drive cannot
    /// keep the "from the internet" mark (`downloads/deliver.rs`); none: it stays in staging.
    fallback: Option<PathBuf>,
    /// The port the host claimed for Chrome's debugging port. Unused since route 2: the browser
    /// has no debugging port (decision record 0060); the host still claims and passes it.
    cdp_port: u16,
    /// Windows only: the port agent-browser's daemon must listen on.
    daemon_port: Option<u16>,
    child_args: Vec<OsString>,
}

/// The Windows folders the child's environment defaults to (`job/spawn.rs`); none elsewhere.
#[cfg(windows)]
fn windows_dirs() -> env::WindowsDirs {
    job::windows_dirs()
}

/// The Windows folders the child's environment defaults to: none off Windows.
#[cfg(not(windows))]
fn windows_dirs() -> env::WindowsDirs {
    env::WindowsDirs::default()
}

/// Runs the guard with the arguments after `browser-guard`. Returns agent-browser's exit code
/// (1 when a signal killed it), or a [`Failure`].
pub fn run(args: &[OsString]) -> Result<i32, Failure> {
    // Linux: the very first act, so as little as can be opened before it (`harden.rs`).
    #[cfg(target_os = "linux")]
    harden::not_dumpable().map_err(|error| {
        failed(&format!(
            "cannot hide the guard's process, so it starts no browser: {error}"
        ))
    })?;
    // First, before anything slow: a parent that dies later is then still the one watched.
    let parent = parent::Parent::now().ok_or_else(|| failed(PARENT_GONE))?;
    // Windows: only the copy created hardened goes on, refusing to when it is not (fail closed);
    // the first process starts it and waits (`harden.rs`).
    #[cfg(windows)]
    let (args, hardened) = match args.split_first() {
        Some((marker, rest)) if marker == harden::HARDENED_COPY => {
            let hardened = harden::Hardened::verified().map_err(|error| {
                failed(&format!(
                    "the guard's copy was not created hardened, so it starts no browser: {error}"
                ))
            })?;
            (rest, hardened)
        }
        _ => return run_hardened_copy(args, &parent),
    };
    let command = parse(args, env::Os::CURRENT).map_err(|reason| Failure {
        code: EXIT_USAGE,
        message: format!("clauseo-mcp browser-guard: {reason}\n{USAGE}"),
    })?;
    let source = env::Source::from_process();
    let refused = |error: env::EnvError| Failure {
        code: EXIT_USAGE,
        message: format!("clauseo-mcp browser-guard: {error}"),
    };
    let env =
        env::build(&source, env::Os::CURRENT, &command.cwd, &windows_dirs()).map_err(refused)?;
    // The guard's alone: read once and its file deleted, the variable never in `env`, the
    // children's map, and both forms of the password redacted from all the guard passes on.
    let secret = env::proxy_secret(&source, env::Os::CURRENT).map_err(refused)?;
    if let Some(password) = &secret {
        proxy_auth::hide(password);
    }
    drop(source);
    // Nothing that reaches agent-browser's command line or `AGENT_BROWSER_ARGS` may name a switch
    // that weakens Chrome's sandbox, though agent-browser never starts Chrome here (`chrome.rs`).
    if let Some(switch) = command
        .child_args
        .iter()
        .chain(env.get(env::ARGS))
        .find_map(|arg| chrome::sandbox::weakening_in(arg))
    {
        return Err(Failure {
            code: EXIT_USAGE,
            message: format!(
                "clauseo-mcp browser-guard: agent-browser's arguments or {} name {switch}, which \
                 weakens Chrome's sandbox",
                env::ARGS
            ),
        });
    }
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
    let mut env = env;
    // The download tool's folder is the mover's: agent-browser's own `download` is refused
    // (`filter.rs`), but nothing names a folder outside the matter for a download.
    env.insert(
        DOWNLOAD_PATH.to_owned(),
        command.downloads.clone().into_os_string(),
    );
    let folders = env::folders(env::Os::CURRENT, &command.cwd);
    for folder in folders.iter().chain([&command.downloads]) {
        write::make_folder(folder).map_err(|error| {
            failed(&format!(
                "cannot make the folder {}: {error}",
                folder.display()
            ))
        })?;
    }
    serve(
        &command,
        env,
        secret,
        &session,
        &parent,
        &run,
        #[cfg(windows)]
        hardened,
    )
}

/// Windows: the guard's first process. Hardens itself, starts its hardened copy with `args` and
/// the same standard handles (`job::start_copy`), and returns the copy's exit code. If this
/// process's parent goes first it exits 1 at once: the copy, which watches this process as its
/// parent, then ends the browser, cleans up and says so on the shared stderr.
#[cfg(windows)]
fn run_hardened_copy(args: &[OsString], parent: &parent::Parent) -> Result<i32, Failure> {
    let hardened = harden::Hardened::apply().map_err(|error| {
        failed(&format!(
            "cannot harden the guard's process, so it starts no browser: {error}"
        ))
    })?;
    let copy = job::start_copy(&hardened, args).map_err(|error| {
        failed(&format!(
            "cannot start the guard's hardened copy, so it starts no browser: {error}"
        ))
    })?;
    drop(hardened);
    let (tell, ended) = std::sync::mpsc::channel();
    let gone = tell.clone();
    parent.watch(move || drop(gone.send(None)));
    // Never joined: it ends with the copy, or with this process.
    drop(std::thread::spawn(move || {
        drop(tell.send(Some(copy.wait())));
    }));
    match ended.recv() {
        Ok(Some(Ok(code))) => Ok(i32::from_ne_bytes(code.to_ne_bytes())),
        Ok(Some(Err(error))) => Err(failed(&format!(
            "cannot wait for the guard's hardened copy: {error}"
        ))),
        Ok(None) | Err(_) => Ok(EXIT_FAILED),
    }
}

/// Relays, starting Chrome and agent-browser at the first tool call (`session.rs`); ends both,
/// whatever happens, and deletes the run's profile before returning.
fn serve(
    command: &Command,
    env: BTreeMap<String, OsString>,
    proxy_secret: Option<proxy_auth::Password>,
    session: &str,
    parent: &parent::Parent,
    run: &runs::Run,
    #[cfg(windows)] hardened: harden::Hardened,
) -> Result<i32, Failure> {
    #[cfg(unix)]
    let socket_dir = env
        .get(env::SOCKET_DIR)
        .map_or_else(PathBuf::new, PathBuf::from);
    #[cfg(unix)]
    let reaper = reaper::Reaper::start(
        &socket_dir,
        session,
        (
            &command.cwd,
            &command.downloads,
            command.fallback.as_deref(),
        ),
        run.id(),
    )
    .map_err(|error| failed(&format!("cannot start the reaper: {error}")))?;
    let launcher = session::Launcher {
        executable: env
            .get(env::EXECUTABLE_PATH)
            .map_or_else(PathBuf::new, PathBuf::from),
        cwd: command.cwd.clone(),
        downloads: command.downloads.clone(),
        fallback: command.fallback.clone(),
        run: run.id().to_owned(),
        profile: run.profile().to_owned(),
        proxy: env.get(env::PROXY).cloned(),
        proxy_secret,
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
        job: job::Job::new(hardened).map_err(|error| failed(&error.to_string()))?,
        started_with: std::sync::Mutex::new(None),
    };
    let shared = Arc::new(session::Shared::new(launcher));
    // Stops whatever runs (Chrome's tree, agent-browser's, the daemon) before it returns.
    let outcome = relay::run(&shared, parent);
    // agent-browser and its daemon are ended: their last stderr lines, redacted, before the exit.
    relay::redact::drain(STDERR_DRAIN);
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
        relay::Outcome::StartFailed(reason) => Err(failed(&reason.to_string())),
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

/// An exit-1 failure with the guard's prefix, redacted (`relay/redact.rs`): a reason can quote
/// Chrome's stderr.
fn failed(reason: &str) -> Failure {
    Failure {
        code: EXIT_FAILED,
        message: format!(
            "clauseo-mcp browser-guard: {}",
            relay::redact::redacted(reason)
        ),
    }
}

/// One line on the guard's stderr, redacted (`relay/redact.rs`): a note can quote Chrome's
/// stderr or a CDP error. Diagnostics only; a failed write has nowhere to go.
pub(crate) fn note(text: &str) {
    let text = relay::redact::redacted(text);
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

/// The port files the relay checks after each child line, on Windows only: the stream server's,
/// which must not exist, and the daemon's, in `<cwd>\run`.
fn port_checks(
    env: &BTreeMap<String, OsString>,
    command: &Command,
    session: &str,
) -> Vec<port_file::Expected> {
    let socket_dir = env
        .get(env::SOCKET_DIR)
        .map_or_else(|| env::windows_socket_dir(&command.cwd), PathBuf::from);
    let mut checks = Vec::new();
    // agent-browser gets no stream port anywhere (route 2). On Windows its server must not run
    // at all; elsewhere an upstream agent-browser may still start one, which is let be.
    if env::Os::CURRENT == env::Os::Windows {
        checks.push(port_file::Expected::stream_off(&socket_dir, session));
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
/// order, with `--downloads <folder>` after `--cwd` or not (and right after it
/// `--fallback-downloads <folder>` or not), and `--daemon-port <port>` after
/// `--cdp-port` on Windows and never on Unix; each path absolute and existing (for
/// `--downloads` and `--fallback-downloads`, its parent: the guard makes the folder), each port
/// from 1024 to 65535 in plain decimal. The child's arguments are kept as given.
fn parse(args: &[OsString], os: env::Os) -> Result<Command, &'static str> {
    let (args, downloads, fallback) = match (args.get(4..6), args.get(6..8)) {
        (Some([flag, folder]), Some([flag_fallback, fallback]))
            if flag == "--downloads" && flag_fallback == "--fallback-downloads" =>
        {
            let mut rest = args.get(..4).unwrap_or_default().to_vec();
            rest.extend_from_slice(args.get(8..).unwrap_or_default());
            (
                rest,
                Some(PathBuf::from(folder)),
                Some(PathBuf::from(fallback)),
            )
        }
        (Some([flag, folder]), _) if flag == "--downloads" => {
            let mut rest = args.get(..4).unwrap_or_default().to_vec();
            rest.extend_from_slice(args.get(6..).unwrap_or_default());
            (rest, Some(PathBuf::from(folder)), None)
        }
        _ => (args.to_vec(), None, None),
    };
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
    ] = args.as_slice()
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
    let downloads = downloads.unwrap_or_else(|| cwd.join("downloads"));
    if !downloads.is_absolute() || !downloads.parent().is_some_and(Path::is_dir) {
        return Err("--downloads must be an absolute path in an existing folder");
    }
    if fallback
        .as_deref()
        .is_some_and(|folder| !folder.is_absolute() || !folder.parent().is_some_and(Path::is_dir))
    {
        return Err("--fallback-downloads must be an absolute path in an existing folder");
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
        downloads,
        fallback,
        cdp_port,
        daemon_port,
        child_args: child_args.to_vec(),
    })
}

#[cfg(test)]
mod tests;
