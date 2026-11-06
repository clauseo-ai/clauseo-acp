//! Unix only: what ends the session's browser processes when the guard cannot.
//!
//! agent-browser's daemon leaves the guard's process group (`setsid`, v0.41.1
//! `connection.rs:944-958`) and is no child of the guard; Chrome leads a group of its own
//! (`chrome.rs`). The guard ends both itself on every way out of [`super::run`]
//! ([`kill_daemon`], then dropping `Chrome`). A guard that is killed (SIGKILL, or a signal it
//! does not handle) or crashes (`panic = "abort"`) cannot, so the guard starts this reaper
//! first: `clauseo-mcp browser-reaper --socket-dir <folder> --session <name> --cwd <folder>
//! --run <id>`, in a process group of its own, with a pipe from the guard on its stdin. Every name
//! it acts on is its own run's (`runs.rs`): the daemon's socket `<socket dir>/run-<id>.sock`
//! (`--session`) and the profile `<cwd>/chrome-profile-<id>`, so a reaper never touches the
//! next run's, which an MCP reconnect starts at once under the same `--cwd`.
//!
//! The reaper first takes its share of the run's lock, so no sweep deletes the run while it may
//! still clean up, then writes `ready`, which the guard waits for before it starts Chrome. The
//! guard writes `chrome <pid>` as soon as Chrome is started (before it waits for Chrome to
//! listen), `agent <pid>` as soon as agent-browser is, `reaped chrome` or `reaped agent`
//! once it has killed or reaped that one itself (the reaper then never signals that pid or
//! group, which could have been reused), and `done` once it has cleaned up. If the pipe ends
//! without `done`, the guard is gone: the reaper kills agent-browser's process group (so no CLI
//! call of it can start a daemon later), the daemon ([`kill_daemon`]) and Chrome's process
//! group, moves Chrome's finished downloads, deletes the profile and the run's lock file. In both
//! cases it ends the run's daemon once more [`DAEMON_AGAIN`] later, which catches a daemon that
//! bound its socket only after the first kill, and exits.
//!
//! The reaper and the guard run in the same jail as the agent, so the reaper can signal nothing
//! the agent could not.

use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use super::chrome::{self, STAGING};
use super::{downloads, pause, runs};
use crate::Failure;

/// The mode's command line.
pub const USAGE: &str = "usage: clauseo-mcp browser-reaper --socket-dir <folder> --session <name> \
                         --cwd <folder> --run <id>";
/// How long the reaper waits for Chrome to be gone before it deletes the profile anyway.
const GONE_WAIT: Duration = Duration::from_secs(5);
/// How long after the guard's `done` or its death the reaper ends the run's daemon once more.
const DAEMON_AGAIN: Duration = Duration::from_secs(1);
/// What the reaper writes once it holds its share of the run's lock.
const READY: &str = "ready";

/// The guard's end of the reaper.
pub(crate) struct Reaper {
    child: Child,
    teller: Teller,
}

/// The reaper's stdin, shared by the guard's threads that tell it something.
#[derive(Clone)]
pub(crate) struct Teller(Arc<Mutex<Option<ChildStdin>>>);

impl Teller {
    /// Writes `line` to the reaper. A reaper gone, or its stdin closed by `done`, is noted.
    pub(crate) fn say(&self, line: &str) {
        let mut stdin = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let sent = stdin.as_mut().is_some_and(|stdin| {
            writeln!(stdin, "{line}")
                .and_then(|()| stdin.flush())
                .is_ok()
        });
        if !sent {
            super::note("the reaper is gone; a killed guard would leave Chrome behind");
        }
    }

    fn close(&self) {
        drop(self.0.lock().unwrap_or_else(PoisonError::into_inner).take());
    }
}

impl Reaper {
    /// Starts the reaper for the run and waits until it holds its share of the run's lock.
    #[expect(
        clippy::disallowed_methods,
        reason = "the guard starts its own binary as its reaper; it starts nothing else"
    )]
    pub(crate) fn start(
        socket_dir: &Path,
        session: &str,
        cwd: &Path,
        run: &str,
    ) -> io::Result<Reaper> {
        use std::os::unix::process::CommandExt;
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("browser-reaper")
            .arg("--socket-dir")
            .arg(socket_dir)
            .args(["--session", session])
            .arg("--cwd")
            .arg(cwd)
            .args(["--run", run])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command.spawn()?;
        let teller = Teller(Arc::new(Mutex::new(child.stdin.take())));
        let mut ready = String::new();
        if let Some(stdout) = child.stdout.take() {
            drop(BufReader::new(stdout).read_line(&mut ready));
        }
        if ready.trim_end() != READY {
            teller.close();
            drop(child.kill());
            drop(child.wait());
            return Err(io::Error::other("the reaper did not start"));
        }
        Ok(Reaper { child, teller })
    }

    /// Tells the reaper Chrome's pid.
    pub(crate) fn chrome(&self, pid: u32) {
        self.teller.say(&format!("chrome {pid}"));
    }

    /// Tells the reaper agent-browser's pid, which also names its process group.
    pub(crate) fn agent(&self, pid: u32) {
        self.teller.say(&format!("agent {pid}"));
    }

    /// Tells the reaper the guard has killed agent-browser's group and reaps agent-browser.
    pub(crate) fn reaped_agent(&self) {
        self.teller.say("reaped agent");
    }

    /// A handle for the threads that tell the reaper something (Chrome's reap hook).
    pub(crate) fn teller(&self) -> Teller {
        self.teller.clone()
    }

    /// Tells the reaper the guard cleaned up and closes the pipe. Not waited for: the reaper
    /// still ends the run's daemon once more [`DAEMON_AGAIN`] later, and the guard exits now;
    /// the reaper's new parent reaps it.
    pub(crate) fn done(self) {
        self.teller.say("done");
        self.teller.close();
        drop(self.child);
    }
}

/// The `browser-reaper` mode, given the arguments after its name.
pub fn run(args: &[OsString]) -> Result<i32, Failure> {
    let usage = || Failure {
        code: 2,
        message: format!("clauseo-mcp browser-reaper: bad command line\n{USAGE}"),
    };
    let [
        flag_dir,
        dir,
        flag_session,
        session,
        flag_cwd,
        cwd,
        flag_run,
        run,
    ] = args
    else {
        return Err(usage());
    };
    if flag_dir != "--socket-dir"
        || flag_session != "--session"
        || flag_cwd != "--cwd"
        || flag_run != "--run"
    {
        return Err(usage());
    }
    let session = session.to_str().ok_or_else(usage)?;
    let run = run
        .to_str()
        .filter(|run| runs::is_id(run))
        .ok_or_else(usage)?;
    let (dir, cwd) = (Path::new(dir), Path::new(cwd));
    // Held until the reaper exits, so no other run's sweep deletes this run meanwhile.
    let hold = runs::hold(cwd, run);
    let mut stdout = io::stdout().lock();
    // A guard that is gone already finds out from its stdin's end below.
    drop(writeln!(stdout, "{READY}").and_then(|()| stdout.flush()));
    drop(stdout);
    let mut pids = Pids::default();
    for line in BufReader::new(io::stdin().lock()).lines() {
        let Ok(line) = line else { break };
        if pids.take(&line) {
            pause(DAEMON_AGAIN);
            kill_daemon(dir, session);
            return Ok(0);
        }
    }
    clean_up(dir, session, cwd, run, pids.chrome, pids.agent);
    pause(DAEMON_AGAIN);
    kill_daemon(dir, session);
    drop(hold);
    Ok(0)
}

/// The pids the guard told the reaper, less the ones it said it has reaped.
#[derive(Debug, Default, PartialEq, Eq)]
struct Pids {
    chrome: Option<u32>,
    agent: Option<u32>,
}

impl Pids {
    /// Takes one line from the guard: `chrome <pid>`, `agent <pid>`, `reaped chrome`,
    /// `reaped agent`; `true` for `done`. Any other line is ignored.
    fn take(&mut self, line: &str) -> bool {
        match line {
            "done" => return true,
            "reaped chrome" => self.chrome = None,
            "reaped agent" => self.agent = None,
            _ => {
                if let Some(pid) = line.strip_prefix("chrome ") {
                    self.chrome = pid.parse().ok();
                }
                if let Some(pid) = line.strip_prefix("agent ") {
                    self.agent = pid.parse().ok();
                }
            }
        }
        false
    }
}

/// What the guard would have done on its way out, for run `run` alone.
fn clean_up(
    socket_dir: &Path,
    session: &str,
    cwd: &Path,
    run: &str,
    chrome_pid: Option<u32>,
    agent_pid: Option<u32>,
) {
    if let Some(pid) = agent_pid {
        chrome::kill_group(pid);
    }
    kill_daemon(socket_dir, session);
    if let Some(pid) = chrome_pid {
        chrome::kill_group(pid);
        // Not the reaper's child, so it cannot be reaped here; its new parent reaps it.
        for _ in 0..GONE_WAIT.as_millis() / 100 {
            if !alive(pid) {
                break;
            }
            pause(Duration::from_millis(100));
        }
    }
    let profile = runs::profile_of(cwd, run);
    downloads::sweep(&profile.join(STAGING), &cwd.join("downloads"));
    chrome::remove_profile(&profile);
    runs::forget(cwd, run);
}

/// SIGKILL to agent-browser's daemon for `session` (the run's own name on Unix, `runs.rs`), if one
/// is running. The pid is not read from
/// the daemon's `<session>.pid` file (a pid from a file can name a reused process, AGENTS.md):
/// the guard connects to the session's socket `<socket dir>/<session>.sock` and asks the kernel
/// which process listens on it (`SO_PEERCRED` on Linux, `LOCAL_PEERPID` on macOS). That process
/// holds the socket open, so it is alive and its pid not reused. No listener, no kill: a daemon
/// that already exited (after the guard's close, or at its idle timeout) is left alone.
pub(crate) fn kill_daemon(socket_dir: &Path, session: &str) {
    let Ok(stream) = UnixStream::connect(socket_dir.join(format!("{session}.sock"))) else {
        return;
    };
    if let Some(pid) = listener_pid(&stream).filter(|pid| *pid > 1) {
        // It may have exited since; nothing is left to kill then.
        if kill(Pid::from_raw(pid), Signal::SIGKILL).is_err() {}
    }
}

/// The pid of the process at the other end of `stream`: the one that listens.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn listener_pid(stream: &UnixStream) -> Option<i32> {
    use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
    getsockopt(stream, PeerCredentials)
        .ok()
        .map(|cred| cred.pid())
}

/// The pid of the process at the other end of `stream`: the one that listens.
#[cfg(target_vendor = "apple")]
fn listener_pid(stream: &UnixStream) -> Option<i32> {
    use nix::sys::socket::{getsockopt, sockopt::LocalPeerPid};
    getsockopt(stream, LocalPeerPid).ok()
}

/// Elsewhere the listener cannot be told; the daemon is left to its idle timeout.
#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn listener_pid(_stream: &UnixStream) -> Option<i32> {
    None
}

/// Whether process `pid` exists (a zombie counts until its parent reaps it).
fn alive(pid: u32) -> bool {
    i32::try_from(pid).is_ok_and(|raw| kill(Pid::from_raw(raw), None).is_ok())
}

#[cfg(test)]
mod tests {
    use super::Pids;

    /// After `reaped chrome` or `reaped agent` the reaper holds no pid for that process, so a
    /// guard killed later during its cleanup leaves the reaper nothing to signal there.
    #[test]
    fn a_pid_the_guard_reaped_is_dropped() {
        let mut pids = Pids::default();
        for line in ["chrome 4100", "agent 4200", "something else"] {
            assert!(!pids.take(line), "{line}");
        }
        let both = Pids {
            chrome: Some(4100),
            agent: Some(4200),
        };
        assert_eq!(pids, both);
        assert!(!pids.take("reaped agent"));
        assert_eq!(pids.agent, None);
        assert_eq!(pids.chrome, Some(4100));
        assert!(!pids.take("reaped chrome"));
        assert_eq!(pids, Pids::default());
        assert!(pids.take("done"));
    }
}
