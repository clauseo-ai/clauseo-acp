//! The session's Chrome, which the guard starts itself before agent-browser and which
//! agent-browser only connects to (`AGENT_BROWSER_CDP`, `env.rs`).
//!
//! agent-browser's own launch cannot use the host's claimed debugging port: it puts
//! `--remote-debugging-port=0` first and waits for the `DevToolsActivePort` file, which Chrome
//! writes only for port 0 (`remote_debugging_server.cc:366-396`), so a pinned port timed out
//! after 30 s (decision record 0029). The guard therefore runs Chrome headless with
//! agent-browser's own headless flags (v0.41.1 `cli/src/native/cdp/chrome.rs:466-597`), the
//! session's proxy, the WebRTC flag and `--remote-debugging-port=<--cdp-port>`; reads Chrome's
//! stderr until `DevTools listening on ws://127.0.0.1:<port>/devtools/browser/<id>` (at most
//! [`START_WAIT`]); and refuses unless the port is exactly the claimed one. The start is two
//! steps, [`launch`] and [`Chrome::endpoint`], so the reaper knows Chrome before the wait.
//!
//! The profile is the run's own, `<cwd>/chrome-profile-<run id>` (`runs.rs`), made fresh and
//! deleted when the guard drops [`Chrome`]. Its `Default/Preferences` sends Chrome's own
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
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use super::downloads::Mover;
use super::parent::{self, Parent};
use super::relay::{Line, Overflow, read_line};
use super::{env, pause, quiet};
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

/// The running Chrome. Dropping it kills Chrome's tree, moves the last finished downloads and
/// deletes the profile.
pub(crate) struct Chrome {
    child: Arc<Mutex<Tracked>>,
    /// Unix: also the id of Chrome's process group.
    #[cfg(unix)]
    pid: u32,
    profile: PathBuf,
    /// Where Chrome downloads to, and where the mover puts each file.
    staging: PathBuf,
    downloads: PathBuf,
    /// What Chrome's stderr reader saw first, until [`Chrome::endpoint`] reads it.
    starting: Option<mpsc::Receiver<Seen>>,
    mover: Option<Mover>,
    /// Dropped to stop the exit watcher.
    stop_watch: Option<mpsc::Sender<()>>,
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
}

/// Starts Chrome, with its stderr reader, and returns it at once, before it listens: the
/// caller hands its pid to the reaper (Unix) before [`Chrome::endpoint`] waits, so a guard
/// killed during the wait leaves no Chrome. On Windows Chrome is in the job before it runs.
pub(crate) fn launch(
    spec: &Spec<'_>,
    #[cfg(windows)] job: &super::job::Job,
) -> Result<Chrome, String> {
    let profile = spec.profile.to_owned();
    let staging = profile.join(STAGING);
    let downloads = spec.cwd.join("downloads");
    fresh_profile(&profile, &staging, &downloads)
        .map_err(|error| format!("cannot make Chrome's profile: {error}"))?;
    let mut child = spawn(spec, &chrome_args(spec, &profile)).map_err(|error| {
        remove_profile(&profile);
        format!("cannot start Chrome: {error}")
    })?;
    #[cfg(windows)]
    job.adopt(&mut child, "Chrome").map_err(|error| {
        drop(child.wait());
        remove_profile(&profile);
        format!("cannot start Chrome: {error}")
    })?;
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
        profile,
        staging,
        downloads,
        starting: Some(inbox),
        mover: None,
        stop_watch: None,
    })
}

impl Chrome {
    /// Waits for Chrome's endpoint, at most [`START_WAIT`], checks it is on the claimed port
    /// and starts the download mover. Every [`parent::POLL`] it also looks whether the guard's
    /// parent has gone, and gives up if so.
    pub(crate) fn endpoint(&mut self, cdp_port: u16, parent: &Parent) -> Result<String, String> {
        let inbox = self
            .starting
            .take()
            .ok_or_else(|| "Chrome's start was already read".to_owned())?;
        let polls = START_WAIT.as_millis() / parent::POLL.as_millis();
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
                    return Err(super::PARENT_GONE.to_owned());
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
        let endpoint = match seen {
            Some(Seen::Endpoint(endpoint)) => endpoint,
            Some(Seen::Failed(tail)) => {
                return Err(format!(
                    "Chrome stopped before it listened on the claimed debugging port{}",
                    quote(&tail)
                ));
            }
            None => {
                return Err(format!(
                    "Chrome did not report its debugging port within {} s",
                    START_WAIT.as_secs()
                ));
            }
        };
        check_endpoint(&endpoint, cdp_port)?;
        self.mover = Some(Mover::start(self.staging.clone(), self.downloads.clone()));
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
        drop(self.stop_watch.take());
        self.kill_tree();
        if let Some(mover) = self.mover.take() {
            mover.finish();
        }
        remove_profile(&self.profile);
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

/// Makes `profile` from nothing, with `Default/Preferences` sending downloads to `staging` and
/// [`quiet::preferences`]'s services off.
///
/// agent-browser's launch let every download through (`Browser.setDownloadBehavior` `allow`);
/// connected to the guard's Chrome it sets `default` (`browser.rs:2563-2574`), so Chrome's own
/// rules apply. One of them holds a download over `http://` of a "high-risk" type (`.md`,
/// `.pdf`, ...) as `*.crdownload` until someone clicks Keep, which never happens headless.
/// Chrome waives that rule where the page's "insecure content" setting is allow, so the profile
/// allows it for `http://` pages only: there no content is mixed, so nothing else changes. A
/// download over `http://` from an `https://` page stays blocked, as in Chrome by default.
fn fresh_profile(profile: &Path, staging: &Path, downloads: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(profile).is_ok() {
        write::remove_dir_all(profile)?;
    }
    write::create_dir_all(&profile.join("Default"))?;
    write::create_dir_all(staging)?;
    let mut preferences = serde_json::json!({
        "download": {
            "default_directory": staging,
            "prompt_for_download": false,
            "directory_upgrade": true,
        },
        "savefile": {"default_directory": downloads},
        "profile": {"content_settings": {"exceptions": {"mixed_script": {
            "http://*:*,*": {"setting": 1},
        }}}},
    });
    if let (Some(own), serde_json::Value::Object(quiet)) =
        (preferences.as_object_mut(), quiet::preferences())
    {
        own.extend(quiet);
    }
    write::write(
        &profile.join("Default").join("Preferences"),
        preferences.to_string(),
    )
}

/// Chrome's arguments, in order.
pub(crate) fn chrome_args(spec: &Spec<'_>, profile: &Path) -> Vec<OsString> {
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
    args.extend(sandbox_args().iter().map(OsString::from));
    args.push(OsString::from("about:blank"));
    args
}

/// agent-browser's rule for Linux (`chrome.rs:1606-1700`): no sandbox and no `/dev/shm` as root
/// or in a container, where Chrome's sandbox cannot start. Its `CI` test does not apply: the
/// guard's child never saw `CI`.
fn sandbox_args() -> &'static [&'static str] {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let root = std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0);
        let container = Path::new("/.dockerenv").exists()
            || Path::new("/run/.containerenv").exists()
            || std::fs::read_to_string("/proc/1/cgroup").is_ok_and(|cgroup| {
                ["docker", "kubepods", "lxc"]
                    .iter()
                    .any(|word| cgroup.contains(word))
            });
        if root || container {
            return &["--no-sandbox", "--disable-dev-shm-usage"];
        }
    }
    &[]
}

#[expect(
    clippy::disallowed_methods,
    reason = "the guard's second fixed child: the host's Chrome, whose tree the guard kills \
              (docs/decisions/0029-browser.md)"
)]
fn spawn(spec: &Spec<'_>, args: &[OsString]) -> io::Result<Child> {
    let mut command = Command::new(spec.executable);
    command
        .args(args)
        .current_dir(spec.cwd)
        .env_clear()
        .envs(spec.env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Suspended until it is in the job (`job.rs`).
        command.creation_flags(super::relay::CREATE_NO_WINDOW | super::job::START_SUSPENDED);
    }
    command.spawn()
}

/// Reads Chrome's stderr to its end: reports the endpoint line, or the last lines if Chrome
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
        if text.contains(CANNOT_LISTEN) {
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

#[cfg(test)]
mod tests;
