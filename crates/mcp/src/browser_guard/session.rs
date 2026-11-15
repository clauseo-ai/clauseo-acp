//! The session's browser, started lazily (decision record 0029, "Lazy start and the idle stop").
//!
//! Nothing runs at the guard's start but the guard (and on Unix its reaper). The agent's first
//! `tools/call` starts the browser ([`start`], from the agent pump in `relay/pumps.rs`, under the
//! state's lock): the guard starts the browser over its `DevTools` pipe with the run's profile,
//! kept from an earlier start (`session/browser.rs`), then agent-browser, given the relay's
//! endpoint and let in by the relay's peer check (route 2, decision record 0060).
//!
//! [`stop`] ends both as the guard's exit does: on Windows the job, waited out and replaced by a
//! new one for the next start (`job.rs`); on Unix agent-browser's
//! process group, the run's daemon (`reaper::kill_daemon`) and Chrome's group, each reaped here
//! and told to the reaper. The profile stays until the guard exits. The relay stops the browser
//! after [`IDLE_STOP`] with no tool call in flight or completed, once the agent's
//! `agent_browser_close` is answered, when Chrome dies the first time (the next call starts it
//! again), and when the guard ends.
//!
//! Each start and each stop moves the generation on. What an incarnation's threads report
//! carries its generation, and the relay ignores what an ended one reports, so a child the guard
//! killed is never taken for one that exited.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, BufReader};
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::chrome::{self, Chrome};
use super::downloads::{Held, Saved};
#[cfg(windows)]
use super::note;
use super::parent::Parent;
use super::port_file::Expected;
use super::proxy_auth::Password;
use super::record::{self, ChromeState};
use super::relay::lines::{Out, ToChild};
use super::relay::{Event, end_group, pumps, send};
use super::{Child, ChildStdin, env, now};

/// How long the browser runs with no tool call in flight or completed before the guard stops
/// it: agent-browser's own idle timeout, `AGENT_BROWSER_IDLE_TIMEOUT_MS` (`env::FIXED`), so the
/// daemon's end and the guard's stop come together rather than one undoing the other. An agent
/// that closes the browser (`agent_browser_close`, `relay.rs`) stops it sooner.
pub(crate) const IDLE_STOP: Duration = Duration::from_secs(600);
/// Tests only (the `test-support` feature, never in a shipped build): the idle stop in
/// milliseconds, read from the guard's own environment.
#[cfg(feature = "test-support")]
pub(crate) const IDLE_STOP_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_IDLE_MS";
/// Tests only (the `test-support` feature, never in a shipped build): Chrome's
/// [`chrome::START_WAIT`] in milliseconds, read from the guard's own environment.
#[cfg(feature = "test-support")]
const START_WAIT_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_START_MS";

/// Why a start failed: its own kind when Chrome's sandbox could not start, so the agent's call
/// gets [`chrome::sandbox::REFUSAL`] and the host can tell it apart later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartError {
    /// Chrome's sandbox could not start; what Chrome printed (the fixed part, no path), or
    /// `None` when the guard refused before starting it (Linux, as root).
    Sandbox(Option<&'static str>),
    /// Anything else; the reason.
    Other(String),
}

impl From<String> for StartError {
    fn from(reason: String) -> Self {
        Self::Other(reason)
    }
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sandbox(Some(said)) => write!(
                f,
                "Chrome's sandbox could not start (Chrome said \"{said}\"); the guard never runs \
                 Chrome without it"
            ),
            Self::Sandbox(None) => f.write_str(
                "Chrome's sandbox cannot start as root; the guard never runs Chrome without it",
            ),
            Self::Other(reason) => f.write_str(reason),
        }
    }
}

/// The idle stop: [`IDLE_STOP`], or in a `test-support` build the test's.
pub(crate) fn idle_stop() -> Duration {
    #[cfg(feature = "test-support")]
    if let Some(ms) = std::env::var(IDLE_STOP_FOR_TESTS)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        return Duration::from_millis(ms);
    }
    IDLE_STOP
}

/// How long a start waits for the browser's answer on its pipe: [`chrome::START_WAIT`], or in a
/// `test-support` build the test's.
fn start_wait() -> Duration {
    #[cfg(feature = "test-support")]
    if let Some(ms) = std::env::var(START_WAIT_FOR_TESTS)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        return Duration::from_millis(ms);
    }
    chrome::START_WAIT
}

/// Everything a start needs, fixed for the guard's life.
pub(crate) struct Launcher {
    /// `AGENT_BROWSER_EXECUTABLE_PATH`.
    pub(crate) executable: PathBuf,
    pub(crate) cwd: PathBuf,
    /// The downloads folder (the guard's `--downloads`), which the mover delivers to.
    pub(crate) downloads: PathBuf,
    /// The guard's `--fallback-downloads`, if given (`downloads/deliver.rs`).
    pub(crate) fallback: Option<PathBuf>,
    /// The run's id and profile (`runs.rs`), kept from one start to the next.
    pub(crate) run: String,
    pub(crate) profile: PathBuf,
    /// `AGENT_BROWSER_PROXY`, if the host gave one.
    pub(crate) proxy: Option<OsString>,
    /// The proxy's password, read once from the file `CLAUSEO_PROXY_SECRET_FILE` names at the
    /// guard's start, and only with the proxy: each start hands it to `seam::clients`, whose
    /// `ProxyAuth` answers Chrome's proxy challenge with it (`proxy_auth.rs`).
    pub(crate) proxy_secret: Option<Password>,
    /// [`env::chrome_env`].
    pub(crate) chrome_env: BTreeMap<String, OsString>,
    /// agent-browser's file and arguments.
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<OsString>,
    /// agent-browser's environment without [`env::CDP`], which each start adds.
    pub(crate) env: BTreeMap<String, OsString>,
    /// Checked after every line agent-browser writes.
    pub(crate) port_checks: Vec<Expected>,
    /// The daemon's socket folder and session name, for `reaper::kill_daemon`.
    #[cfg(unix)]
    pub(crate) socket_dir: PathBuf,
    #[cfg(unix)]
    pub(crate) session: String,
    /// [`idle_stop`].
    pub(crate) idle: Duration,
    #[cfg(unix)]
    pub(crate) reaper: super::reaper::Teller,
    #[cfg(windows)]
    pub(crate) job: super::job::Job,
    /// The browser that started last, tried first next time (`session/browser.rs`).
    pub(crate) started_with: Mutex<Option<PathBuf>>,
}

/// Why a start failed: the guard's line (its own kind for Chrome's sandbox), and words for the
/// agent's tool call beyond the plain "could not start" (on Windows, when no browser is installed
/// or every one is blocked).
#[derive(Debug)]
pub(crate) struct StartFailure {
    pub(crate) reason: StartError,
    pub(crate) agent_note: Option<&'static str>,
}

impl From<StartError> for StartFailure {
    fn from(reason: StartError) -> Self {
        StartFailure {
            reason,
            agent_note: None,
        }
    }
}

impl From<String> for StartFailure {
    fn from(reason: String) -> Self {
        StartError::Other(reason).into()
    }
}

#[cfg(all(test, unix))]
impl Launcher {
    /// A launcher for the Unix unit tests that drive the pumps: nothing in it is ever started.
    pub(crate) fn for_tests(dir: &std::path::Path, port_checks: Vec<Expected>) -> Launcher {
        Launcher {
            executable: dir.join("chrome"),
            cwd: dir.to_owned(),
            downloads: dir.join("downloads"),
            fallback: None,
            run: "1-0000abcd".to_owned(),
            profile: dir.join("chrome-profile-1-0000abcd"),
            started_with: Mutex::new(None),
            proxy: Some(OsString::from("http://127.0.0.1:9")),
            proxy_secret: None,
            chrome_env: BTreeMap::new(),
            program: dir.join("agent-browser"),
            args: Vec::new(),
            env: BTreeMap::new(),
            port_checks,
            socket_dir: dir.to_owned(),
            session: "s".to_owned(),
            idle: IDLE_STOP,
            reaper: super::reaper::Teller::none(),
        }
    }
}

/// What the guard's threads share.
pub(crate) struct Shared {
    pub(crate) launcher: Launcher,
    pub(crate) state: Mutex<State>,
    /// The guard's stdout.
    pub(crate) out: Out,
    /// Set once the agent has closed stdin and the close request is going out.
    pub(crate) closing: AtomicBool,
    /// The one-shot agent-browser answering a request while the browser is stopped
    /// (`relay/oneshot.rs`), until it is reaped.
    pub(crate) oneshot: Mutex<Option<Child>>,
    /// The downloads Chrome holds for confirmation, which the agent has not been told about yet
    /// (`downloads/held.rs`); kept across starts.
    pub(crate) held: Arc<Held>,
    /// The files the mover delivered, which the agent has not been told about yet
    /// (`downloads/saved.rs`); kept across starts.
    pub(crate) saved: Arc<Saved>,
}

impl Shared {
    pub(crate) fn new(launcher: Launcher) -> Shared {
        let saved = Arc::new(Saved::queued_in(launcher.cwd.clone()));
        Shared {
            launcher,
            state: Mutex::new(State {
                generation: 0,
                live: None,
                in_flight: Vec::new(),
                sent: 0,
                idle_since: now(),
                restarted: false,
                domain_restarts: 0,
                sandbox_refused: None,
                closing: false,
                ended: false,
            }),
            out: Mutex::new(io::stdout()),
            closing: AtomicBool::new(false),
            oneshot: Mutex::new(None),
            held: Arc::default(),
            saved,
        }
    }
}

/// What runs now, and the requests it has not answered.
pub(crate) struct State {
    /// Moved on by every start and stop.
    pub(crate) generation: u64,
    /// Chrome and agent-browser, while they run.
    pub(crate) live: Option<Live>,
    /// The requests sent to the running agent-browser and not yet answered: each id, and what
    /// it is.
    in_flight: Vec<(Value, Request)>,
    /// How many requests have gone to agent-browser in the guard's life: a close's stop is
    /// skipped once another has gone since its reply was taken (`relay.rs`).
    sent: u64,
    /// When the browser started or last answered a tool call.
    idle_since: Instant,
    /// Whether Chrome has died once already; a second death ends the guard.
    pub(crate) restarted: bool,
    /// How often the guard has stopped the browser for agent-browser's refusal of
    /// `--allowed-domains` (`relay/pumps.rs`): at most once per run.
    pub(crate) domain_restarts: u32,
    /// Set once a running Chrome said its sandbox cannot start (`relay.rs`): no browser starts
    /// again, and each later call gets the sandbox's refusal; what Chrome said.
    pub(crate) sandbox_refused: Option<&'static str>,
    /// Set once the agent has closed stdin: nothing starts or idles any more.
    pub(crate) closing: bool,
    /// Set by the relay's end, under this lock, before its final [`stop`]: from then on no
    /// request starts the browser or a one-shot agent-browser; each gets an error instead.
    pub(crate) ended: bool,
}

/// What a request sent to the running agent-browser is, for its reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    /// A tool call but those below: its reply carries the download notes.
    Call,
    /// An `agent_browser_open` call: its reply, when the URL turned into a download, says the
    /// file was saved (`downloads/saved.rs`).
    Open,
    /// An `agent_browser_tools_profiles` call: its reply loses the mention of JavaScript eval
    /// (`filter::without_eval`).
    Profiles,
    /// An `agent_browser_close` call: its reply stops the browser (`relay.rs`).
    Close,
    /// `tools/list`: its reply loses the tools and argument keys the guard refuses
    /// (`filter::advertised_tools`).
    List,
    /// Any other request (`ping`), relayed as read.
    Other,
}

impl Request {
    /// Whether it is a tool call, whose reply restarts the idle time.
    pub(crate) fn is_call(self) -> bool {
        matches!(
            self,
            Request::Call | Request::Close | Request::Open | Request::Profiles
        )
    }
}

/// The running Chrome and agent-browser.
pub(crate) struct Live {
    chrome: Chrome,
    agent: Child,
    pub(crate) to_child: ToChild,
}

/// What [`stop`] found and did.
pub(crate) struct Stopped {
    /// agent-browser's exit status, once reaped.
    pub(crate) status: io::Result<ExitStatus>,
    /// Whether Chrome had exited by itself before the stop.
    pub(crate) chrome_died: bool,
}

impl State {
    /// Notes request `id` sent to the running agent-browser.
    pub(crate) fn sent(&mut self, id: Value, request: Request) {
        self.in_flight.push((id, request));
        self.sent += 1;
    }

    /// How many requests have gone to agent-browser so far.
    pub(crate) fn sent_count(&self) -> u64 {
        self.sent
    }

    /// Takes the reply to `id` off the list, and returns what it answers (`None` for no request
    /// in flight). A tool call's reply restarts the idle time.
    pub(crate) fn answered(&mut self, id: &Value) -> Option<Request> {
        let at = self.in_flight.iter().position(|(sent, _)| sent == id)?;
        let (_, request) = self.in_flight.remove(at);
        if request.is_call() {
            self.idle_since = now();
        }
        Some(request)
    }

    /// Whether the browser runs with nothing in flight and the agent still there.
    pub(crate) fn quiet(&self) -> bool {
        self.live.is_some() && self.in_flight.is_empty() && !self.closing
    }

    /// Whether the stop after an `agent_browser_close` reply may run (`relay.rs`): the browser
    /// is [`State::quiet`] and no request has gone to agent-browser since the close's reply was
    /// taken, when [`State::sent_count`] read `sent`. A call sent and answered in between would
    /// leave the browser quiet, and the stop would end the Chrome that call just used.
    pub(crate) fn close_may_stop(&self, sent: u64) -> bool {
        self.quiet() && self.sent == sent
    }

    /// The ids still unanswered, which the guard now answers itself.
    pub(crate) fn take_in_flight(&mut self) -> Vec<Value> {
        self.in_flight.drain(..).map(|(id, _)| id).collect()
    }

    /// How long until the browser has idled for `idle`: `None` unless it runs with nothing in
    /// flight and the agent is still there.
    pub(crate) fn idle_left(&self, idle: Duration) -> Option<Duration> {
        self.quiet()
            .then(|| idle.saturating_sub(self.idle_since.elapsed()))
    }
}

/// Starts Chrome and then agent-browser into `state`, which has neither. Whatever of it started
/// is ended again when a step fails, and the run's Chrome record says `none` again; the profile
/// stays.
pub(crate) fn start(
    shared: &Arc<Shared>,
    state: &mut State,
    parent: &Parent,
    events: &mpsc::Sender<Event>,
) -> Result<(), StartFailure> {
    // A Chrome whose sandbox failed is never started again, nor any other browser.
    if let Some(said) = state.sandbox_refused {
        return Err(StartError::Sandbox(Some(said)).into());
    }
    let started = start_both(shared, state, parent, events);
    if started.is_err() {
        let launcher = &shared.launcher;
        record::write_state(&launcher.cwd, &launcher.run, &ChromeState::None);
    }
    started
}

/// [`start`]'s steps; a Chrome started here is killed when a later step returns early.
fn start_both(
    shared: &Arc<Shared>,
    state: &mut State,
    parent: &Parent,
    events: &mpsc::Sender<Event>,
) -> Result<(), StartFailure> {
    let launcher = &shared.launcher;
    let (mut chrome, endpoint) = browser::start_browser(shared, parent)?;
    let mut env = launcher.env.clone();
    env::set_cdp(&mut env, &endpoint);
    let cannot_run =
        |error: io::Error| format!("cannot run {}: {error}", launcher.program.display());
    let mut agent = pumps::spawn_child(
        &launcher.program,
        &launcher.args,
        &launcher.cwd,
        &env,
        #[cfg(windows)]
        &launcher.job,
    )
    .map_err(cannot_run)?;
    #[cfg(unix)]
    launcher.reaper.say(&format!("agent {}", agent.id()));
    // agent-browser and its descendants (its daemon) may connect to the relay; nothing else.
    chrome.allow_client(agent.id());
    let (Some(stdin), Some(stdout)) = (agent.stdin.take(), agent.stdout.take()) else {
        drop(end_agent(launcher, &mut agent));
        return Err(cannot_run(io::Error::other("its stdin or stdout is not piped")).into());
    };
    state.generation += 1;
    let generation = state.generation;
    {
        let events = events.clone();
        chrome.watch(move || send(&events, Event::ChromeExited(generation)));
    }
    {
        let events = events.clone();
        chrome.on_sandbox_failure(move |said| {
            send(&events, Event::SandboxFailed(generation, said));
        });
    }
    {
        let (shared, events) = (Arc::clone(shared), events.clone());
        // Never joined: it sends its end as an event, and a grandchild of agent-browser's could
        // still hold the pipe open.
        drop(thread::spawn(move || {
            let end = pumps::pump_child(BufReader::new(stdout), &shared, generation, &events);
            send(&events, end);
        }));
    }
    state.live = Some(Live {
        chrome,
        agent,
        to_child: Arc::new(Mutex::new(Some::<ChildStdin>(stdin))),
    });
    state.idle_since = now();
    Ok(())
}

/// Ends the running Chrome and agent-browser, if any, as the guard's exit does. The generation
/// moves on first, so what the ended incarnation's threads report next is ignored.
pub(crate) fn stop(launcher: &Launcher, state: &mut State) -> Option<Stopped> {
    state.generation += 1;
    let Live {
        chrome, mut agent, ..
    } = state.live.take()?;
    // Before anything here ends Chrome (the job's end on Windows): a Chrome gone now died by
    // itself.
    let chrome_died = chrome.exited();
    // Returns once the job's processes are gone (their files closed), and leaves a new job for
    // the next start, which Windows would refuse in the ended one (`job.rs`).
    #[cfg(windows)]
    if !launcher.job.kill() {
        note("Windows refused to end the guard's job; ending agent-browser and Chrome alone");
    }
    let status = end_agent(launcher, &mut agent);
    #[cfg(unix)]
    super::reaper::kill_daemon(&launcher.socket_dir, &launcher.session);
    // Kills Chrome's tree (telling the reaper) and moves the last downloads; the profile stays.
    drop(chrome);
    record::write_state(&launcher.cwd, &launcher.run, &ChromeState::None);
    Some(Stopped {
        status,
        chrome_died,
    })
}

/// Ends agent-browser's group, tells the reaper, and reaps agent-browser.
pub(crate) fn end_agent(
    #[cfg_attr(windows, expect(unused_variables, reason = "Windows has no reaper"))]
    launcher: &Launcher,
    agent: &mut Child,
) -> io::Result<ExitStatus> {
    end_group(agent);
    #[cfg(unix)]
    launcher.reaper.say("reaped agent");
    agent.wait()
}

mod browser;

#[cfg(all(test, unix))]
mod tests {
    use serde_json::json;

    use super::{Request, Shared};

    /// A call sent and answered after a close's reply was taken leaves nothing in flight, but
    /// moves the count on, so the close's stop is skipped (`relay.rs`'s `stop_after_close`):
    /// without the count the stop would end the Chrome that call had just used.
    #[test]
    fn a_call_answered_after_the_close_moves_the_count_on() {
        let dir = std::env::temp_dir();
        let shared = Shared::new(super::Launcher::for_tests(&dir, Vec::new()));
        let mut state = shared.state.lock().unwrap();
        state.sent(json!("close"), Request::Close);
        assert_eq!(state.answered(&json!("close")), Some(Request::Close));
        let at_close = state.sent_count();
        assert_eq!(state.in_flight.len(), 0);
        state.sent(json!("open"), Request::Call);
        assert_eq!(state.answered(&json!("open")), Some(Request::Call));
        assert_eq!(state.in_flight.len(), 0, "the open is answered");
        assert_ne!(state.sent_count(), at_close, "the open went unnoticed");
    }
}
