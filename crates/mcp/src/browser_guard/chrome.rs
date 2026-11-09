//! The session's Chrome, which the guard starts itself before agent-browser, at the first tool
//! call and again after each stop (`session.rs`), and which agent-browser only connects to
//! (`AGENT_BROWSER_CDP`, `env.rs`).
//!
//! agent-browser's own launch cannot use the host's claimed debugging port: it puts
//! `--remote-debugging-port=0` first and waits for the `DevToolsActivePort` file, which Chrome
//! writes only for port 0 (`remote_debugging_server.cc:366-396`), so a pinned port timed out
//! after 30 s (decision record 0029). The guard therefore runs Chrome headless with
//! agent-browser's own headless flags (v0.41.1 `cli/src/native/cdp/chrome.rs:466-597`), the
//! session's proxy, the WebRTC flag and `--remote-debugging-port=<--cdp-port>`, never a switch
//! that weakens its sandbox (`chrome/sandbox.rs`, refused when that sandbox cannot start); reads
//! Chrome's stderr until `DevTools listening on ws://127.0.0.1:<port>/devtools/browser/<id>` (at
//! most [`START_WAIT`]); and refuses unless the port is exactly the claimed one. The start is two
//! steps, [`launch`] and [`Chrome::endpoint`], so the reaper knows Chrome before the wait.
//!
//! The profile is the run's own, `<cwd>/chrome-profile-<run id>` (`runs.rs`), made at the first
//! start and kept by every later one, so tabs, cookies and storage outlive an idle stop or a
//! crash; the guard deletes it when it exits. Its `Default/Preferences` sends Chrome's own
//! downloads to a staging folder inside it, from which `downloads.rs` moves each finished file
//! into `<cwd>/downloads`.
//!
//! Chrome is the guard's child: on Unix the leader of its own process group, which [`Chrome`]'s
//! drop kills whole; on Windows in the guard's Job Object (`job.rs`). The guard drops it on
//! every way out of [`super::run`], and the reaper (`reaper.rs`, Unix) or the job (Windows)
//! covers a guard that is killed or crashes. Whatever reaps Chrome here first tells the reaper
//! ([`Chrome::on_reap`]), so the reaper never signals a pid the guard has reaped.

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use super::downloads::{Held, Mover};
use super::parent::{self, Parent};
use super::relay::{Line, Overflow, read_line};
use super::session::{Refused, StartError};
use super::{Child, ChildStderr, env, pause, quiet};
use crate::write;

/// How long Chrome has to print its `DevTools` endpoint.
pub(crate) const START_WAIT: Duration = Duration::from_secs(15);
/// The folder in the profile Chrome downloads into, before `downloads.rs` moves each file.
pub(crate) const STAGING: &str = "downloads-staging";
/// How often the watcher looks whether Chrome has exited.
const WATCH_EVERY: Duration = Duration::from_millis(250);
/// The longest stderr line kept; longer ones are skipped.
const MAX_STDERR_LINE: usize = 64 * 1024;
/// How many of Chrome's last stderr lines a start failure quotes.
const TAIL_LINES: usize = 5;
/// The line Chrome prints once its `DevTools` server listens (`devtools_http_handler.cc:293`).
const LISTENING: &str = "DevTools listening on ";
/// The line Chrome prints when the port is taken (`devtools_http_handler.cc`).
const CANNOT_LISTEN: &str = "Cannot start http server for devtools";

/// agent-browser v0.41.1's flags for a headless launch with no extensions, webgpu or real
/// keychain (`chrome.rs:466-560`), less its `--remote-debugging-port=0` and its
/// `--disable-features` and `--enable-features`, which [`quiet::FEATURES`] extends.
const HEADLESS_ARGS: &[&str] = &[
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-background-networking",
    "--disable-backgrounding-occluded-windows",
    "--disable-component-update",
    "--disable-default-apps",
    "--disable-hang-monitor",
    "--disable-popup-blocking",
    "--disable-prompt-on-repost",
    "--disable-sync",
    "--metrics-recording-only",
    "--password-store=basic",
    "--use-mock-keychain",
    "--headless=new",
    "--hide-scrollbars",
    "--enable-unsafe-swiftshader",
    "--window-size=1280,720",
];

/// What [`launch`] needs.
pub(crate) struct Spec<'a> {
    /// `AGENT_BROWSER_EXECUTABLE_PATH`, checked by `env.rs`.
    pub(crate) executable: &'a Path,
    pub(crate) cwd: &'a Path,
    /// The run's profile (`runs.rs`), which [`launch`] makes.
    pub(crate) profile: &'a Path,
    /// `--cdp-port`.
    pub(crate) cdp_port: u16,
    /// `AGENT_BROWSER_PROXY`, checked by `env.rs`.
    pub(crate) proxy: &'a OsStr,
    /// [`env::chrome_env`].
    pub(crate) env: &'a BTreeMap<String, OsString>,
}

/// The running Chrome. Dropping it kills Chrome's tree and moves the last finished downloads;
/// the profile stays.
pub(crate) struct Chrome {
    child: Arc<Mutex<Tracked>>,
    /// Unix: also the id of Chrome's process group.
    #[cfg(unix)]
    pid: u32,
    /// Where Chrome downloads to, and where the mover puts each file.
    staging: PathBuf,
    downloads: PathBuf,
    /// What Chrome's stderr reader saw first, until [`Chrome::endpoint`] reads it.
    starting: Option<mpsc::Receiver<Seen>>,
    mover: Option<Mover>,
    /// Dropped to stop the exit watcher and the sandbox's sentinel (`postcheck/sentinel.rs`).
    stops: Vec<mpsc::Sender<()>>,
}

/// Chrome's process, and what to call once it is reaped here.
struct Tracked {
    child: Child,
    on_reap: Option<Box<dyn FnOnce() + Send>>,
}

impl Tracked {
    /// Whether Chrome has exited (reaped here if so), without waiting.
    fn exited(&mut self) -> bool {
        let exited = !matches!(self.child.try_wait(), Ok(None));
        if exited {
            self.reaped();
        }
        exited
    }

    /// Calls the reap hook, once.
    fn reaped(&mut self) {
        if let Some(tell) = self.on_reap.take() {
            tell();
        }
    }
}

/// What Chrome's stderr reader saw first.
enum Seen {
    Endpoint(String),
    /// Chrome closed stderr, or said it cannot listen, first; its last lines.
    Failed(Vec<String>),
    /// Chrome said its sandbox cannot start, first: the fixed part of what it said
    /// ([`sandbox::failure`]).
    Sandbox(&'static str),
}

/// Starts Chrome, with its stderr reader, and returns it at once, before it listens: the
/// caller hands its pid to the reaper (Unix) before [`Chrome::endpoint`] waits, so a guard
/// killed during the wait leaves no Chrome. On Windows Chrome is in the job before it runs.
/// Refuses, starting nothing, on Linux as root (where Chrome's sandbox cannot start), on Windows
/// in a job that limits windows, and when an argument would weaken Chrome's sandbox
/// (`sandbox.rs`).
pub(crate) fn launch(
    spec: &Spec<'_>,
    #[cfg(windows)] job: &super::job::Job,
) -> Result<Chrome, StartError> {
    launch_on(
        spec,
        sandbox::Host::now(),
        #[cfg(windows)]
        job,
    )
}

/// [`launch`] on `host`.
fn launch_on(
    spec: &Spec<'_>,
    host: sandbox::Host,
    #[cfg(windows)] job: &super::job::Job,
) -> Result<Chrome, StartError> {
    if sandbox::refused_up_front(host) {
        return Err(StartError::Sandbox(Refused::Root));
    }
    // Chrome's sandbox nests UI-restricted jobs, which Windows refuses below any job with UI
    // restrictions (`job/probe.rs`).
    #[cfg(windows)]
    if super::job::ui_jobs_nest() == super::job::Nesting::Refused {
        return Err(StartError::Sandbox(Refused::JobLimitsWindows));
    }
    let profile = spec.profile.to_owned();
    let staging = profile.join(STAGING);
    let downloads = spec.cwd.join("downloads");
    ready_profile(&profile, &staging, &downloads)
        .map_err(|error| format!("cannot make Chrome's profile: {error}"))?;
    let args = chrome_args(spec, &profile, host);
    if let Some(switch) = args.iter().find_map(|arg| sandbox::weakening(arg)) {
        return Err(StartError::Other(format!(
            "Chrome's arguments include {switch}, which weakens its sandbox; the guard never \
             passes it"
        )));
    }
    #[cfg(feature = "test-support")]
    let args = sandbox::unsandboxed_for_tests(args);
    let mut child = spawn(
        spec,
        &args,
        #[cfg(windows)]
        job,
    )
    .map_err(|error| format!("cannot start Chrome: {error}"))?;
    let (seen, inbox) = mpsc::channel();
    if let Some(stderr) = child.stderr.take() {
        drop(thread::spawn(move || read_stderr(stderr, &seen)));
    }
    Ok(Chrome {
        #[cfg(unix)]
        pid: child.id(),
        child: Arc::new(Mutex::new(Tracked {
            child,
            on_reap: None,
        })),
        staging,
        downloads,
        starting: Some(inbox),
        mover: None,
        stops: Vec::new(),
    })
}

impl Chrome {
    /// Waits for Chrome's endpoint, at most `wait` ([`START_WAIT`] but in tests), checks it is on
    /// the claimed port and starts the download mover, which records held downloads in `held`.
    /// Every [`parent::POLL`] it also looks whether the guard's parent has gone, and gives up if
    /// so. On Windows a Chrome that exits first with a crash's code, or a sandboxed child of it
    /// in `job` that does so during the wait, is the sandbox's refusal (`postcheck/crashed.rs`).
    pub(crate) fn endpoint(
        &mut self,
        cdp_port: u16,
        wait: Duration,
        parent: &Parent,
        held: Arc<Held>,
        #[cfg(windows)] job: &super::job::Job,
    ) -> Result<String, StartError> {
        let inbox = self
            .starting
            .take()
            .ok_or_else(|| "Chrome's start was already read".to_owned())?;
        let polls = wait.as_millis() / parent::POLL.as_millis();
        #[cfg(windows)]
        let mut watch = postcheck::WaitWatch::new(job, lock(&self.child).child.id());
        let mut seen = None;
        for _ in 0..polls {
            match inbox.recv_timeout(parent::POLL) {
                Ok(first) => {
                    seen = Some(first);
                    break;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    seen = Some(Seen::Failed(Vec::new()));
                    break;
                }
                Err(RecvTimeoutError::Timeout) if parent.gone() => {
                    return Err(super::PARENT_GONE.to_owned().into());
                }
                Err(RecvTimeoutError::Timeout) => {
                    #[cfg(windows)]
                    watch.sample();
                }
            }
        }
        let endpoint = match seen {
            Some(Seen::Endpoint(endpoint)) => endpoint,
            Some(Seen::Sandbox(said)) => return Err(StartError::Sandbox(Refused::Said(said))),
            #[cfg(windows)]
            Some(Seen::Failed(_))
                if postcheck::crashed_at_start(self) || watch.sandbox_failed() =>
            {
                return Err(StartError::Sandbox(Refused::Crashed));
            }
            Some(Seen::Failed(tail)) => {
                return Err(StartError::Other(format!(
                    "Chrome stopped before it listened on the claimed debugging port{}",
                    quote(&tail)
                )));
            }
            #[cfg(windows)]
            None if watch.sandbox_failed() => {
                return Err(StartError::Sandbox(Refused::Crashed));
            }
            None => {
                return Err(StartError::Other(format!(
                    "Chrome did not report its debugging port within {} s",
                    wait.as_secs()
                )));
            }
        };
        check_endpoint(&endpoint, cdp_port)?;
        self.mover = Some(Mover::start(
            self.staging.clone(),
            self.downloads.clone(),
            held,
        ));
        Ok(endpoint)
    }

    /// Whether Chrome has exited (and been reaped here), without waiting.
    pub(crate) fn exited(&self) -> bool {
        lock(&self.child).exited()
    }

    /// Calls `tell` once Chrome is reaped here, or, when the guard kills it, just before: the
    /// guard tells the reaper, which then never signals Chrome's pid or group.
    #[cfg(unix)]
    pub(crate) fn on_reap(&mut self, tell: impl FnOnce() + Send + 'static) {
        lock(&self.child).on_reap = Some(Box::new(tell));
    }

    /// Chrome's process id, which also names its process group.
    #[cfg(unix)]
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    /// What the run's Chrome record says once Chrome is spawned (`record.rs`): on Unix its group,
    /// on Windows its pid and creation time, or `starting` (read as alive) when that time cannot
    /// be read.
    pub(crate) fn recorded(&self) -> super::record::ChromeState {
        use super::record::{ChromeProcess, ChromeState};
        #[cfg(unix)]
        {
            ChromeState::Running(ChromeProcess {
                id: self.pid,
                created: 0,
            })
        }
        #[cfg(windows)]
        {
            let tracked = lock(&self.child);
            match super::record::windows::created(&tracked.child) {
                Some(created) => ChromeState::Running(ChromeProcess {
                    id: tracked.child.id(),
                    created,
                }),
                None => ChromeState::Starting,
            }
        }
    }

    /// Calls `on_exit` once if Chrome exits before this value drops.
    pub(crate) fn watch(&mut self, on_exit: impl FnOnce() + Send + 'static) {
        let (stop, stopped) = mpsc::channel::<()>();
        self.stops.push(stop);
        let child = Arc::clone(&self.child);
        drop(thread::spawn(move || {
            loop {
                match stopped.recv_timeout(WATCH_EVERY) {
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                }
                if lock(&child).exited() {
                    on_exit();
                    return;
                }
            }
        }));
    }

    /// Kills Chrome's tree now, as its drop will (the sentinel's refusal, `session.rs`).
    pub(crate) fn kill(&self) {
        self.kill_tree();
    }

    /// Kills Chrome's tree, if Chrome still runs, and reaps Chrome. The process group is
    /// signalled only while its leader is unreaped (under the lock the watcher also takes), so
    /// its id cannot have been reused. The reap hook runs after the kill, before the reap.
    fn kill_tree(&self) {
        let mut tracked = lock(&self.child);
        if !tracked.exited() {
            #[cfg(unix)]
            kill_group(self.pid);
            #[cfg(not(unix))]
            drop(tracked.child.kill());
            tracked.reaped();
        }
        drop(tracked.child.wait());
    }
}

impl Drop for Chrome {
    fn drop(&mut self) {
        self.stops.clear();
        self.kill_tree();
        if let Some(mover) = self.mover.take() {
            mover.finish();
        }
    }
}

/// SIGKILL to process group `pid`. A group already gone is not an error.
#[cfg(unix)]
pub(crate) fn kill_group(pid: u32) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    if let Ok(raw) = i32::try_from(pid)
        && raw > 1
    {
        // Gone already is fine: nothing is left to kill.
        if killpg(Pid::from_raw(raw), Signal::SIGKILL).is_err() {}
    }
}

/// Deletes the profile, retrying for a second while Chrome's last helpers let go of it.
pub(crate) fn remove_profile(profile: &Path) {
    for _ in 0..10 {
        match write::remove_dir_all(profile) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                pause(Duration::from_millis(100));
            }
            _ => return,
        }
    }
    super::note(&format!(
        "could not delete Chrome's profile {}",
        profile.display()
    ));
}

/// Chrome's arguments, in order, on `host`: never one that weakens Chrome's sandbox
/// (`sandbox.rs`).
pub(crate) fn chrome_args(spec: &Spec<'_>, profile: &Path, host: sandbox::Host) -> Vec<OsString> {
    let mut args: Vec<OsString> = HEADLESS_ARGS.iter().map(OsString::from).collect();
    args.extend(
        quiet::FEATURES
            .iter()
            .chain(quiet::ARGS)
            .map(OsString::from),
    );
    let mut proxy = OsString::from("--proxy-server=");
    proxy.push(spec.proxy);
    args.push(proxy);
    // `env::FIXED`'s bypass: none, not even loopback.
    args.push(OsString::from("--proxy-bypass-list=<-loopback>"));
    let mut user_data = OsString::from("--user-data-dir=");
    user_data.push(profile);
    args.push(user_data);
    args.push(OsString::from(env::WEBRTC_ARG));
    args.push(OsString::from(format!(
        "--remote-debugging-port={}",
        spec.cdp_port
    )));
    args.extend(sandbox::host_args(host).iter().map(OsString::from));
    args.push(OsString::from("about:blank"));
    args
}

#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "the guard's second fixed child: the host's Chrome, whose tree the guard kills \
              (docs/decisions/0029-browser.md)"
)]
fn spawn(spec: &Spec<'_>, args: &[OsString]) -> io::Result<Child> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut command = Command::new(spec.executable);
    command
        .args(args)
        .current_dir(spec.cwd)
        .env_clear()
        .envs(spec.env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .process_group(0);
    command.spawn()
}

/// Windows: Chrome as on Unix, created in the guard's job (`job/spawn.rs`).
#[cfg(windows)]
fn spawn(spec: &Spec<'_>, args: &[OsString], job: &super::job::Job) -> io::Result<Child> {
    use super::job::{Io, Launch};
    job.spawn(&Launch {
        program: spec.executable,
        args,
        cwd: spec.cwd,
        env: spec.env,
        stdin: Io::Null,
        stdout: Io::Null,
        stderr: Io::Piped,
    })
}

/// Reads Chrome's stderr to its end: reports the endpoint line, a sandbox failure
/// ([`sandbox::failure`]; Chrome is refused even if it would go on), or the last lines if Chrome
/// fails first, then drains the rest so Chrome never blocks on a full pipe.
fn read_stderr(stderr: ChildStderr, seen: &mpsc::Sender<Seen>) {
    let mut input = BufReader::new(stderr);
    let mut line = Vec::new();
    let mut tail = VecDeque::new();
    let mut reported = false;
    loop {
        match read_line(&mut input, &mut line, MAX_STDERR_LINE, Overflow::Skip) {
            Ok(Line::Whole | Line::Partial) => {}
            Ok(Line::Oversize) => continue,
            Ok(Line::Eof) | Err(_) => break,
        }
        if reported {
            continue;
        }
        let text = String::from_utf8_lossy(&line);
        if let Some(endpoint) = text.trim().strip_prefix(LISTENING) {
            reported = true;
            drop(seen.send(Seen::Endpoint(endpoint.to_owned())));
            continue;
        }
        if tail.len() == TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(text.trim().to_owned());
        if let Some(said) = sandbox::failure(&text) {
            reported = true;
            drop(seen.send(Seen::Sandbox(said)));
        } else if text.contains(CANNOT_LISTEN) {
            reported = true;
            drop(seen.send(Seen::Failed(tail.iter().cloned().collect())));
        }
    }
    if !reported {
        drop(seen.send(Seen::Failed(tail.into_iter().collect())));
    }
}

/// Exactly `ws://127.0.0.1:<port>/devtools/browser/<id>`, `<id>` 1 to 64 of `A-Z a-z 0-9 -`.
/// The refusal names the port, never the endpoint (which grants control of the browser).
pub(crate) fn check_endpoint(endpoint: &str, port: u16) -> Result<(), String> {
    let refused = || "Chrome reported a debugging endpoint the guard does not accept".to_owned();
    let rest = endpoint
        .strip_prefix("ws://127.0.0.1:")
        .ok_or_else(refused)?;
    let (seen, path) = rest.split_once('/').ok_or_else(refused)?;
    if seen != port.to_string() {
        return Err(format!(
            "Chrome listens on debugging port {seen}, not the claimed port {port}"
        ));
    }
    let id = path.strip_prefix("devtools/browser/").ok_or_else(refused)?;
    let valid =
        (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if valid { Ok(()) } else { Err(refused()) }
}

fn quote(tail: &[String]) -> String {
    if tail.is_empty() {
        String::new()
    } else {
        format!("; its last stderr lines: {}", tail.join(" | "))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) mod postcheck;
mod profile;
pub(crate) mod sandbox;

pub(super) use profile::ready_profile;

#[cfg(test)]
mod tests;
