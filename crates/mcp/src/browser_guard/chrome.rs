//! The session's browser, which the guard starts itself before agent-browser, at the first tool
//! call and again after each stop (`session.rs`), and which agent-browser only reaches through
//! the guard's relay (`AGENT_BROWSER_CDP`, `env.rs`, `devtools.rs`).
//!
//! The browser has no debugging port on any system (route 2, decision record 0060): a port on
//! 127.0.0.1 has no authentication, and a sandboxed command can reach loopback. The guard runs
//! it headless with agent-browser's own headless flags (v0.41.1
//! `cli/src/native/cdp/chrome.rs:466-597`), the session's proxy and the WebRTC flag, never a
//! switch that weakens its sandbox (`chrome/sandbox.rs`, refused when that sandbox cannot start),
//! and with `--remote-debugging-pipe`: `DevTools` over a pipe whose ends the guard holds
//! (`chrome/pipe.rs`; fds 3 and 4 on macOS and Linux, inherited handles on Windows). The browser is
//! the host's Chrome on macOS and Linux, and Edge or a Program Files Chrome on Windows
//! (`browsers.rs`). The start is two steps, [`launch`] and [`Chrome::endpoint`], so the
//! reaper knows Chrome before the wait for its answer on the pipe (at most [`START_WAIT`]).
//!
//! The proxy (`--proxy-server`, with no credential: Chrome ignores one there) is in every entry
//! the host makes, the plain client's Codex entry included (decision record 0053). The proxy
//! asks for its password, which the guard supplies as its own client on the browser's `DevTools`
//! pipe ([`Chrome::start_guard`], `proxy_auth.rs`), before agent-browser connects.
//!
//! The profile is the run's own, `<cwd>/chrome-profile-<run id>` (`runs.rs`), made at the first
//! start and kept by every later one, so tabs, cookies and storage outlive an idle stop or a
//! crash; the guard deletes it when it exits. Its `Default/Preferences` sends Chrome's own
//! downloads to a staging folder inside it, from which `downloads.rs` moves each finished file
//! into the downloads folder (the guard's `--downloads`).
//!
//! Chrome is the guard's child: on Unix the leader of its own process group, which [`Chrome`]'s
//! drop kills whole; on Windows in the guard's Job Object (`job.rs`). The guard drops it on
//! every way out of [`super::run`], and the reaper (`reaper.rs`, Unix) or the job (Windows)
//! covers a guard that is killed or crashes. Whatever reaps Chrome here first tells the reaper
//! ([`Chrome::on_reap`]), so the reaper never signals a pid the guard has reaped.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use super::downloads::{Held, Mover, Saved};
use super::parent::Parent;
use super::proxy_auth::Password;
use super::session::StartError;
use super::{Child, env, pause, quiet};
use crate::write;

/// How long the browser has to answer on its `DevTools` pipe.
pub(crate) const START_WAIT: Duration = Duration::from_secs(15);
/// The folder in the profile Chrome downloads into, before `downloads.rs` moves each file.
pub(crate) const STAGING: &str = "downloads-staging";
/// How long a stop waits for Chrome's process group to be gone, as the reaper does (`reaper.rs`).
#[cfg(unix)]
const GROUP_WAIT: Duration = Duration::from_secs(5);
/// How often that wait looks.
#[cfg(unix)]
const GROUP_LOOK: Duration = Duration::from_millis(20);
/// How often the watcher looks whether Chrome has exited.
const WATCH_EVERY: Duration = Duration::from_millis(250);

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
    /// The downloads folder the mover delivers to: `<matter>/downloads` in a Codex session.
    pub(crate) downloads: &'a Path,
    /// The run's profile (`runs.rs`), which [`launch`] makes.
    pub(crate) profile: &'a Path,
    /// `AGENT_BROWSER_PROXY`, checked by `env.rs`; none when the host gave none.
    pub(crate) proxy: Option<&'a OsStr>,
    /// [`env::chrome_env`].
    pub(crate) env: &'a BTreeMap<String, OsString>,
}

/// The running Chrome. Dropping it kills Chrome's tree, waits (Unix) until no process of its group
/// is left, and moves the last finished downloads; the profile stays.
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
    /// Dropped to stop the exit watcher.
    stop_watch: Option<mpsc::Sender<()>>,
    /// Where the stderr reader tells a sandbox failure after the start
    /// ([`Chrome::on_sandbox_failure`]).
    later: Arc<Later>,
    /// The relay holding the guard's ends of the browser's `DevTools` pipe. Dropped after the
    /// browser is killed.
    devtools: super::devtools::Devtools,
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

/// Starts Chrome, with its stderr reader, and returns it at once, before it listens: the
/// caller hands its pid to the reaper (Unix) before [`Chrome::endpoint`] waits, so a guard
/// killed during the wait leaves no Chrome. On Windows Chrome is in the job before it runs.
/// Refuses, starting nothing, on Linux as root (where Chrome's sandbox cannot start) and when an
/// argument would weaken Chrome's sandbox (`sandbox.rs`).
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
        return Err(StartError::Sandbox(None));
    }
    let profile = spec.profile.to_owned();
    let staging = profile.join(STAGING);
    let downloads = spec.downloads.to_owned();
    ready_profile(&profile, &staging, &downloads)
        .map_err(|error| format!("cannot make Chrome's profile: {error}"))?;
    // The arguments but for Windows' pipe handles, which name no switch.
    if let Some(switch) = chrome_args(spec, &profile, host, None)
        .iter()
        .find_map(|arg| sandbox::weakening(arg))
    {
        return Err(StartError::Other(format!(
            "Chrome's arguments include {switch}, which weakens its sandbox; the guard never \
             passes it"
        )));
    }
    let (mut child, devtools) = pipe::spawn(
        spec,
        &profile,
        host,
        #[cfg(windows)]
        job,
    )
    .map_err(|error| format!("cannot start {}: {error}", spec.executable.display()))?;
    let (seen, inbox) = mpsc::channel();
    let later = Arc::new(Later::default());
    if let Some(stderr) = child.stderr.take() {
        let later = Arc::clone(&later);
        // Never joined: it ends at the end of Chrome's stderr, once Chrome's group is gone.
        drop(thread::spawn(move || read_stderr(stderr, &seen, &later)));
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
        stop_watch: None,
        later,
        devtools,
    })
}

impl Chrome {
    /// Waits for the browser's answer on its pipe, at most `wait` ([`START_WAIT`] but in tests),
    /// starts the download mover, which records held downloads in `held` and delivered ones in
    /// `saved`, and returns the relay's endpoint for agent-browser. Every [`super::parent::POLL`]
    /// it also looks whether the guard's parent has gone, and gives up if so.
    pub(crate) fn endpoint(
        &mut self,
        wait: Duration,
        parent: &Parent,
        (held, saved): (Arc<Held>, Arc<Saved>),
    ) -> Result<String, StartError> {
        let inbox = self
            .starting
            .take()
            .ok_or_else(|| "the browser's start was already read".to_owned())?;
        pipe::wait_ready(&self.devtools, &inbox, wait, parent)?;
        self.start_mover(held, saved);
        Ok(self.devtools.url())
    }

    /// Attaches the guard's own CDP clients and sends their start commands
    /// (`devtools/seam.rs`), before agent-browser starts; `env` is agent-browser's, and
    /// `password` the proxy's, if the guard read one (`proxy_auth.rs`).
    pub(crate) fn start_guard(
        &self,
        env: &BTreeMap<String, OsString>,
        password: Option<&Password>,
    ) -> Result<(), String> {
        self.devtools.start_guard(env, password)
    }

    /// Lets agent-browser, the guard's child `pid`, and its descendants connect to the relay
    /// (`devtools/peer.rs`).
    pub(crate) fn allow_client(&self, pid: u32) {
        self.devtools.allow_descendants_of(pid);
    }

    /// Starts the download mover, which records held downloads in `held` and delivered ones
    /// in `saved`.
    fn start_mover(&mut self, held: Arc<Held>, saved: Arc<Saved>) {
        self.mover = Some(Mover::start(
            self.staging.clone(),
            self.downloads.clone(),
            held,
            saved,
        ));
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
        self.stop_watch = Some(stop);
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

    /// Calls `tell` once if Chrome says its sandbox cannot start after its start was accepted
    /// (or said so just before, too late for the start's check), before this value drops.
    pub(crate) fn on_sandbox_failure(&self, tell: impl FnOnce(&'static str) + Send + 'static) {
        self.later.set(Box::new(tell));
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
        drop(tracked);
        #[cfg(unix)]
        self.wait_for_group();
    }

    /// Returns once no process is left in Chrome's group, at most [`GROUP_WAIT`]. Chrome's
    /// helpers (its zygotes, GPU process, broker, utility services) outlive a browser process
    /// that died by itself, which the guard then does not signal (its group's id may be reused
    /// once it is reaped), and still write to the profile until they notice and exit; a group the
    /// guard killed takes a moment to go too. Before this wait the guard deleted the profile
    /// while one still ran, and it wrote the folder back: the real-Chrome test
    /// `calls_after_the_final_stop_start_no_chrome` found the profile left after the guard's exit
    /// (CI #907, 2026-11-10). Only probes the group (`kill(-pgid, 0)`), never signals it.
    #[cfg(unix)]
    fn wait_for_group(&self) {
        let group = super::record::ChromeProcess {
            id: self.pid,
            created: 0,
        };
        for _ in 0..GROUP_WAIT.as_millis() / GROUP_LOOK.as_millis() {
            if group.gone() {
                return;
            }
            pause(GROUP_LOOK);
        }
        super::note("Chrome's helpers were still running when the guard stopped waiting for them");
    }
}

impl Drop for Chrome {
    fn drop(&mut self) {
        drop(self.stop_watch.take());
        self.later.clear();
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

/// The browser's arguments, in order, on `host`: never one that weakens Chrome's sandbox
/// (`sandbox.rs`). `io_pipes` are the pipe's two inherited handle values on Windows
/// (`--remote-debugging-io-pipes=<read>,<write>`); elsewhere none, the browser using fds 3 and 4.
pub(crate) fn chrome_args(
    spec: &Spec<'_>,
    profile: &Path,
    host: sandbox::Host,
    io_pipes: Option<(u32, u32)>,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = HEADLESS_ARGS.iter().map(OsString::from).collect();
    args.extend(
        quiet::FEATURES
            .iter()
            .chain(quiet::ARGS)
            .map(OsString::from),
    );
    // Every host entry gives the proxy now; a guard started without one (an older host) lets
    // Chrome go direct.
    if let Some(given) = spec.proxy {
        let mut proxy = OsString::from("--proxy-server=");
        proxy.push(given);
        args.push(proxy);
        // `env::FIXED`'s bypass: none, not even loopback.
        args.push(OsString::from("--proxy-bypass-list=<-loopback>"));
    }
    let mut user_data = OsString::from("--user-data-dir=");
    user_data.push(profile);
    args.push(user_data);
    args.push(OsString::from(env::WEBRTC_ARG));
    args.push(OsString::from("--remote-debugging-pipe"));
    if let Some((read, write)) = io_pipes {
        args.push(OsString::from(format!(
            "--remote-debugging-io-pipes={read},{write}"
        )));
    }
    args.extend(sandbox::host_args(host).iter().map(OsString::from));
    args.push(OsString::from("about:blank"));
    args
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

mod pipe;
mod profile;
pub(crate) mod sandbox;
mod stderr;

pub(super) use profile::ready_profile;
use stderr::{Later, Seen, quote, read_stderr};

#[cfg(test)]
mod tests;
