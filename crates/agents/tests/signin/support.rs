//! The fixture every sign-in test starts from: `Agents` on the embedded snapshot, with Claude's
//! install taken as ready (`ready_for_tests`) and its adapter played by `fake_agent` (a script
//! named `claude-cli` picks its Claude CLI mode); a clearing launcher that records each run's
//! arguments and pid; a timer whose waits the test ends one by one, by duration; and a fixed
//! clock. The base environment holds the four remote hints, which must never reach the child.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use clauseo_agents::testing::{TempDir, installed, write_file};
use clauseo_agents::{
    Agents, AgentsConfig, AgentsEvent, Clock, InstallState, NodeSearch, Platform, SigninState,
};
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions, Timer};
use clauseo_protocol::AgentId;
use futures::channel::oneshot;

/// The waits of a Claude sign-in.
pub(crate) const POLL: Duration = Duration::from_millis(1500);
pub(crate) const STATUS_LIMIT: Duration = Duration::from_secs(5);
pub(crate) const NUDGE: Duration = Duration::from_secs(30);
pub(crate) const LOGIN_LIMIT: Duration = Duration::from_secs(600);

/// The remote hints the test's own environment sets, which no child may see.
pub(crate) const REMOTE_HINTS: [&str; 4] = [
    "SSH_CONNECTION",
    "SSH_TTY",
    "NO_BROWSER",
    "CLAUDE_CODE_REMOTE",
];

/// The address the fake's login calls `$BROWSER` with, and the one it prints for the paste
/// route.
pub(crate) const BROWSER_URL: &str = "https://claude.com/cai/oauth/authorize?fake=1";
pub(crate) const PRINTED_URL: &str = "https://claude.com/cai/oauth/authorize?redirect_uri=https://platform.claude.com/oauth/code/callback";

pub(crate) fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

pub(crate) fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

/// The fixed time the clock reads.
pub(crate) fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_hours(500_000)
}

#[derive(Debug)]
struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        now()
    }

    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

/// Blocks the test thread for a moment without `thread::sleep` (banned): a wait on a condvar
/// nobody signals. Used only while waiting for something another thread or the OS does.
pub(crate) fn pause() {
    let lock = Mutex::new(());
    let guard = lock.lock().unwrap();
    drop(Condvar::new().wait_timeout(guard, Duration::from_millis(10)));
}

/// How many [`pause`]s a wait for another thread may take before the test fails: 30 s.
const PATIENCE: u32 = 3000;

/// One wait the code under test armed.
struct Wait {
    duration: Duration,
    fire: Option<oneshot::Sender<()>>,
}

/// A timer whose waits end only when the test fires them, picked by duration, so one wait
/// (the nudge) can end while the others (the polls, the 10 minutes) stay armed.
#[derive(Clone, Default)]
pub(crate) struct KeyedTimer {
    waits: Arc<(Mutex<Vec<Wait>>, Condvar)>,
}

impl Timer for KeyedTimer {
    fn after(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        let (fire, fired) = oneshot::channel();
        let (waits, armed) = &*self.waits;
        lock(waits).push(Wait {
            duration,
            fire: Some(fire),
        });
        armed.notify_all();
        async move {
            // A wait the test never fires never ends.
            if fired.await.is_err() {
                futures::future::pending::<()>().await;
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl KeyedTimer {
    /// Every wait armed so far, in order.
    pub(crate) fn armed(&self) -> Vec<Duration> {
        lock(&self.waits.0)
            .iter()
            .map(|wait| wait.duration)
            .collect()
    }

    /// The waits still armed: not fired, and their future not dropped.
    pub(crate) fn live(&self) -> Vec<Duration> {
        lock(&self.waits.0)
            .iter()
            .filter(|wait| wait.fire.as_ref().is_some_and(|fire| !fire.is_canceled()))
            .map(|wait| wait.duration)
            .collect()
    }

    /// Blocks until a wait of `duration` is live, then ends the oldest such.
    pub(crate) fn fire(&self, duration: Duration) {
        let (waits, armed) = &*self.waits;
        let mut guard = lock(waits);
        for _ in 0..PATIENCE {
            let live = guard.iter_mut().find(|wait| {
                wait.duration == duration
                    && wait.fire.as_ref().is_some_and(|fire| !fire.is_canceled())
            });
            if let Some(wait) = live {
                let fire = wait.fire.take().unwrap();
                // The future may have just been dropped; then nothing waits on it.
                fire.send(()).unwrap_or_default();
                return;
            }
            guard = armed
                .wait_timeout(guard, Duration::from_millis(10))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        panic!("no {duration:?} wait was armed; armed: {:?}", self.armed());
    }
}

/// One run the launcher started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Launch {
    pub(crate) args: Vec<String>,
    pub(crate) env: BTreeMap<OsString, OsString>,
    pub(crate) pid: u32,
}

impl Launch {
    /// Whether this was a run of `auth <word>`.
    pub(crate) fn is(&self, word: &str) -> bool {
        self.args
            .windows(2)
            .any(|pair| pair[0] == "auth" && pair[1] == word)
    }
}

/// The passthrough launcher with a cleared environment, as R20 must give, recording each run.
struct Recording {
    inner: PassthroughLauncher,
    launches: Arc<Mutex<Vec<Launch>>>,
}

impl Launcher for Recording {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        let launched = self.inner.launch(spec)?;
        lock(&self.launches).push(Launch {
            args: spec
                .args()
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            env: spec.env().clone(),
            pid: launched.handle.pid(),
        });
        Ok(launched)
    }
}

/// One test's folders, config, timer and launches.
pub(crate) struct Fixture {
    pub(crate) data: TempDir,
    pub(crate) config: AgentsConfig,
    pub(crate) timer: KeyedTimer,
    launches: Arc<Mutex<Vec<Launch>>>,
}

impl Fixture {
    /// The fixture with `FAKE_SIGNIN` set to `script`.
    pub(crate) fn new(label: &str, script: &str) -> Fixture {
        let data = TempDir::new(label).unwrap();
        let root = data.path();
        let bin = clauseo_fake_agent::bin_path()
            .expect("build fake_agent first: cargo build -p clauseo-fake_agent");
        let cli = root.join("adapter").join("claude-cli");
        write_file(&cli, b"").unwrap();
        let mut config = AgentsConfig::new(root.to_path_buf(), Platform::current().unwrap());
        config.node_search = NodeSearch::none();
        config.clock = Arc::new(FixedClock);
        config.ready_for_tests.insert(
            claude(),
            installed(bin, cli, root.join("adapter"), "0.0.0-test"),
        );
        config
            .signin
            .homes
            .insert(claude(), root.join("agents/claude-acp/home"));
        let mut base: BTreeMap<OsString, OsString> = BTreeMap::new();
        let mut put = |name: &str, value: &Path| {
            base.insert(name.into(), value.as_os_str().to_owned());
        };
        put("HOME", &root.join("user-home"));
        put("FAKE_SIGNIN_ENV_OUT", &root.join("env-names"));
        put("FAKE_SIGNIN", Path::new(script));
        put("SSH_CONNECTION", Path::new("10.0.0.1 50000 10.0.0.2 22"));
        put("SSH_TTY", Path::new("/dev/pts/9"));
        put("NO_BROWSER", Path::new("1"));
        put("CLAUDE_CODE_REMOTE", Path::new("true"));
        if let Some(path) = std::env::var_os("PATH") {
            base.insert("PATH".into(), path);
        }
        config.signin.base_env = base;
        Fixture {
            data,
            config,
            timer: KeyedTimer::default(),
            launches: Arc::default(),
        }
    }

    /// Starts `Agents` and waits for Claude's install to report `Ready`.
    pub(crate) fn start(&self) -> (Agents, Events) {
        let launcher: Arc<dyn Launcher> = Arc::new(Recording {
            inner: PassthroughLauncher::new(SpawnOptions::new().clear_env()),
            launches: Arc::clone(&self.launches),
        });
        let (agents, events) = Agents::start(self.config.clone(), launcher, self.timer.clone());
        let events = Events(events);
        events.until_install_ready();
        (agents, events)
    }

    /// Every run started so far.
    pub(crate) fn launches(&self) -> Vec<Launch> {
        lock(&self.launches).clone()
    }

    /// The runs of `auth <word>` started so far.
    pub(crate) fn runs_of(&self, word: &str) -> Vec<Launch> {
        self.launches()
            .into_iter()
            .filter(|run| run.is(word))
            .collect()
    }

    /// The names the fake's last run found in its environment.
    pub(crate) fn env_names(&self) -> Vec<String> {
        let text = std::fs::read_to_string(self.data.path().join("env-names")).unwrap();
        text.lines().map(str::to_owned).collect()
    }

    /// `<data_dir>/agents/signin`.
    pub(crate) fn signin_folder(&self) -> PathBuf {
        self.data.path().join("agents/signin")
    }

    /// The URL files in the sign-in folder now (every file but the shim).
    pub(crate) fn url_files(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.signin_folder()) else {
            return Vec::new();
        };
        entries
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.file_name().is_some_and(|name| name != "browser"))
            .collect()
    }
}

/// A sign-in event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Seen {
    State(SigninState),
    Open(String),
}

/// The events' receiver, read by blocking. Install events are skipped.
pub(crate) struct Events(pub(crate) async_channel::Receiver<AgentsEvent>);

impl Events {
    fn until_install_ready(&self) {
        loop {
            let event = futures::executor::block_on(self.0.recv()).expect("the events closed");
            if let AgentsEvent::Install { agent, state } = event
                && agent == claude()
            {
                assert!(matches!(state, InstallState::Ready { .. }), "{state:?}");
                return;
            }
        }
    }

    /// The next sign-in event for Claude.
    pub(crate) fn next(&self) -> Seen {
        loop {
            let event = futures::executor::block_on(self.0.recv()).expect("the events closed");
            match event {
                AgentsEvent::Signin { agent, state } if agent == claude() => {
                    return Seen::State(state);
                }
                AgentsEvent::OpenUrl { agent, url } if agent == claude() => return Seen::Open(url),
                _ => {}
            }
        }
    }

    /// Events up to and including the first state for which `done` holds.
    pub(crate) fn until(&self, done: impl Fn(&SigninState) -> bool) -> Vec<Seen> {
        let mut seen = Vec::new();
        loop {
            let next = self.next();
            let last = matches!(&next, Seen::State(state) if done(state));
            seen.push(next);
            if last {
                return seen;
            }
        }
    }

    /// Events up to the run's last state (`SignedIn`, `SignedOut` or `Failed`), and that state.
    pub(crate) fn until_end(&self) -> (Vec<Seen>, SigninState) {
        let seen = self.until(|state| {
            matches!(
                state,
                SigninState::SignedIn { .. }
                    | SigninState::SignedOut { .. }
                    | SigninState::Failed { .. }
            )
        });
        let Some(Seen::State(last)) = seen.last().cloned() else {
            unreachable!()
        };
        (seen, last)
    }

    /// Every event already sent, without waiting, Install events included.
    pub(crate) fn drain(&self) -> Vec<AgentsEvent> {
        std::iter::from_fn(|| self.0.try_recv().ok()).collect()
    }
}

/// Whether the process `pid` is gone (or a zombie no one has reaped), after a short wait.
#[cfg(target_os = "linux")]
pub(crate) fn gone(pid: u32) -> bool {
    for _ in 0..PATIENCE {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Err(_) => return true,
            Ok(stat) => {
                // The state follows the command name's closing parenthesis.
                let state = stat.rsplit(')').next().unwrap_or_default().trim_start();
                if state.starts_with(['Z', 'X']) {
                    return true;
                }
            }
        }
        pause();
    }
    false
}

/// Waits until `check` holds, or fails the test after 30 s.
pub(crate) fn eventually(what: &str, check: impl Fn() -> bool) {
    for _ in 0..PATIENCE {
        if check() {
            return;
        }
        pause();
    }
    panic!("never: {what}");
}
