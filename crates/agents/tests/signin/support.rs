//! The fixture every sign-in test starts from: `Agents` on the embedded snapshot, with Claude's
//! and Codex's installs taken as ready (`ready_for_tests`) and their CLIs played by `fake_agent`
//! (a script named `claude-cli` picks its Claude CLI mode, one named `codex.js` its Codex mode),
//! Codex's callback port moved to one that is always free (port 0); a clearing launcher that
//! records each run's arguments and pid; a timer whose waits the test ends one by one, by
//! duration; a fixed clock; and a test resolver, online until the test says otherwise, which
//! records each name looked up. The base environment holds the four remote hints, which must
//! never reach the child.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use clauseo_agents::testing::{TempDir, installed, write_file};
use clauseo_agents::{
    Agents, AgentsConfig, AgentsEvent, Clock, InstallState, Installed, NodeSearch, Platform,
    SigninNote, SigninState, TestResolver,
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
    /// The Codex command after the script and its leading `-c <value>` overrides.
    pub(crate) fn command(&self) -> Vec<&str> {
        let mut args: Vec<&str> = self.args.iter().skip(1).map(String::as_str).collect();
        while args.first() == Some(&"-c") && args.len() >= 2 {
            args.drain(..2);
        }
        args
    }

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
    online: Arc<AtomicBool>,
    lookups: Arc<Mutex<Vec<String>>>,
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
        let codex_js = root.join("codex").join("codex.js");
        write_file(&codex_js, b"").unwrap();
        let mut config = AgentsConfig::new(root.to_path_buf(), Platform::current().unwrap());
        config.node_search = NodeSearch::none();
        config.clock = Arc::new(FixedClock);
        config.ready_for_tests.insert(
            claude(),
            installed(bin.clone(), cli, root.join("adapter"), "0.0.0-test"),
        );
        config.ready_for_tests.insert(
            codex(),
            installed(bin, codex_js, root.join("codex"), "0.0.0-test"),
        );
        config
            .signin
            .homes
            .insert(claude(), root.join("agents/claude-acp/home"));
        config
            .signin
            .homes
            .insert(codex(), root.join("agents/codex-acp/home"));
        config.signin.callback_port = Some(0);
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
        let online = Arc::new(AtomicBool::new(true));
        let lookups: Arc<Mutex<Vec<String>>> = Arc::default();
        let (up, seen) = (Arc::clone(&online), Arc::clone(&lookups));
        config.install.resolver = Some(TestResolver(Arc::new(move |name: &str| {
            lock(&seen).push(name.to_owned());
            if up.load(Ordering::SeqCst) {
                Ok(vec![([127, 0, 0, 1], 0).into()])
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "offline"))
            }
        })));
        Fixture {
            data,
            config,
            timer: KeyedTimer::default(),
            launches: Arc::default(),
            online,
            lookups,
        }
    }

    /// Takes `agent`'s install out of `ready_for_tests`, so it starts `Missing`, and gives the
    /// install [`Agents::finish_install_for_tests`] can make `Ready` later.
    pub(crate) fn not_installed(&mut self, agent: &AgentId) -> Installed {
        self.config.ready_for_tests.remove(agent).unwrap()
    }

    /// Whether the resolver answers (online) or fails every lookup (offline).
    pub(crate) fn set_online(&self, online: bool) {
        self.online.store(online, Ordering::SeqCst);
    }

    /// The names looked up so far.
    pub(crate) fn lookups(&self) -> Vec<String> {
        lock(&self.lookups).clone()
    }

    /// Starts `Agents` and waits for Claude's install to report `Ready`.
    pub(crate) fn start(&self) -> (Agents, Events) {
        self.start_for(claude())
    }

    /// Starts `Agents` and waits for `agent`'s install to report `Ready`; the events are
    /// `agent`'s.
    pub(crate) fn start_for(&self, agent: AgentId) -> (Agents, Events) {
        let (agents, events) = self.start_quiet(agent);
        events.until_install_ready();
        (agents, events)
    }

    /// Starts `Agents` and waits for `agent`'s first install state, whatever it is.
    pub(crate) fn start_missing(&self, agent: AgentId) -> (Agents, Events) {
        let (agents, events) = self.start_quiet(agent);
        let first = events.install_state();
        assert!(matches!(first, InstallState::Missing { .. }), "{first:?}");
        (agents, events)
    }

    fn start_quiet(&self, agent: AgentId) -> (Agents, Events) {
        let launcher: Arc<dyn Launcher> = Arc::new(Recording {
            inner: PassthroughLauncher::new(SpawnOptions::new().clear_env()),
            launches: Arc::clone(&self.launches),
        });
        let (agents, events) = Agents::start(self.config.clone(), launcher, self.timer.clone());
        let events = Events {
            rx: events,
            agent,
            notes: Mutex::default(),
        };
        (agents, events)
    }

    /// Every run started so far.
    pub(crate) fn launches(&self) -> Vec<Launch> {
        lock(&self.launches).clone()
    }

    /// The Codex runs of `command` (after the overrides) started so far.
    pub(crate) fn codex_runs(&self, command: &[&str]) -> Vec<Launch> {
        self.launches()
            .into_iter()
            .filter(|run| run.command() == command)
            .collect()
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

    /// The lawyer's home folder: the base environment's `HOME`.
    pub(crate) fn user_home(&self) -> PathBuf {
        self.data.path().join("user-home")
    }

    /// `<data_dir>/agents/<id>/<name>`: Clauseo's own markers for `agent`.
    pub(crate) fn marker(&self, agent: &AgentId, name: &str) -> PathBuf {
        self.data
            .path()
            .join("agents")
            .join(agent.as_str())
            .join(name)
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

/// One agent's events, read by blocking. Install events and other agents' are skipped; the
/// agent's sign-in notes are kept aside ([`Events::notes`]).
pub(crate) struct Events {
    rx: async_channel::Receiver<AgentsEvent>,
    agent: AgentId,
    notes: Mutex<Vec<SigninNote>>,
}

impl Events {
    fn until_install_ready(&self) {
        let state = self.install_state();
        assert!(matches!(state, InstallState::Ready { .. }), "{state:?}");
    }

    /// The agent's next install state; sign-in events before it are dropped.
    pub(crate) fn install_state(&self) -> InstallState {
        loop {
            let event = futures::executor::block_on(self.rx.recv()).expect("the events closed");
            if let AgentsEvent::Install { agent, state } = event
                && agent == self.agent
            {
                return state;
            }
        }
    }

    /// The next sign-in event for the agent.
    pub(crate) fn next(&self) -> Seen {
        loop {
            let event = futures::executor::block_on(self.rx.recv()).expect("the events closed");
            match event {
                AgentsEvent::Signin { agent, state } if agent == self.agent => {
                    return Seen::State(state);
                }
                AgentsEvent::OpenUrl { agent, url } if agent == self.agent => {
                    return Seen::Open(url);
                }
                AgentsEvent::SigninNote { agent, note } if agent == self.agent => {
                    lock(&self.notes).push(note);
                }
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

    /// The next sign-in state for the agent, `OpenUrl` skipped.
    pub(crate) fn state(&self) -> SigninState {
        loop {
            if let Seen::State(state) = self.next() {
                return state;
            }
        }
    }

    /// The agent's sign-in notes read so far.
    pub(crate) fn notes(&self) -> Vec<SigninNote> {
        lock(&self.notes).clone()
    }

    /// Every event already sent, without waiting, Install events included.
    pub(crate) fn drain(&self) -> Vec<AgentsEvent> {
        std::iter::from_fn(|| self.rx.try_recv().ok()).collect()
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
