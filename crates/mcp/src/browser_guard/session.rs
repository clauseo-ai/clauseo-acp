//! The session's browser, started lazily (decision record 0029, "Lazy start and the idle stop").
//!
//! Nothing runs at the guard's start but the guard (and on Unix its reaper). The agent's first
//! `tools/call` starts the browser ([`start`], from the agent pump in `relay/pumps.rs`, under the
//! state's lock): the guard waits up to [`PORT_WAIT`] for the claimed debugging port to be free
//! (the run an MCP reconnect replaced may still be ending its Chrome there), starts Chrome on it
//! with the run's profile, kept from an earlier start, then agent-browser connected to it.
//!
//! [`stop`] ends both as the guard's exit does: on Windows the job; on Unix agent-browser's
//! process group, the run's daemon (`reaper::kill_daemon`) and Chrome's group, each reaped here
//! and told to the reaper. The profile stays until the guard exits. The relay stops the browser
//! after [`IDLE_STOP`] with no tool call in flight or completed, when Chrome dies the first time
//! (the next call starts it again), and when the guard ends.
//!
//! Each start and each stop moves the generation on. What an incarnation's threads report
//! carries its generation, and the relay ignores what an ended one reports, so a child the guard
//! killed is never taken for one that exited.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, BufReader};
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ExitStatus};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::chrome::{self, Chrome, Spec};
use super::parent::{self, Parent};
use super::port_file::Expected;
use super::record::{self, ChromeState};
use super::relay::lines::{Out, ToChild};
use super::relay::{Event, end_group, pumps, send};
use super::{PARENT_GONE, env, note, now, pause};

/// How long the browser runs with no tool call in flight or completed before the guard stops
/// it: agent-browser's own idle timeout, `AGENT_BROWSER_IDLE_TIMEOUT_MS` (`env::FIXED`), so the
/// daemon's end and the guard's stop come together rather than one undoing the other.
pub(crate) const IDLE_STOP: Duration = Duration::from_secs(600);
/// Tests only (the `test-support` feature, never in a shipped build): the idle stop in
/// milliseconds, read from the guard's own environment.
#[cfg(feature = "test-support")]
pub(crate) const IDLE_STOP_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_IDLE_MS";
/// How long a start waits for the claimed debugging port to be free.
pub(crate) const PORT_WAIT: Duration = Duration::from_secs(15);

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

/// Everything a start needs, fixed for the guard's life.
pub(crate) struct Launcher {
    /// `AGENT_BROWSER_EXECUTABLE_PATH`.
    pub(crate) executable: PathBuf,
    pub(crate) cwd: PathBuf,
    /// The run's id and profile (`runs.rs`), kept from one start to the next.
    pub(crate) run: String,
    pub(crate) profile: PathBuf,
    pub(crate) cdp_port: u16,
    /// `AGENT_BROWSER_PROXY`.
    pub(crate) proxy: OsString,
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
}

#[cfg(all(test, unix))]
impl Launcher {
    /// A launcher for the Unix unit tests that drive the pumps: nothing in it is ever started.
    pub(crate) fn for_tests(dir: &std::path::Path, port_checks: Vec<Expected>) -> Launcher {
        Launcher {
            executable: dir.join("chrome"),
            cwd: dir.to_owned(),
            run: "1-0000abcd".to_owned(),
            profile: dir.join("chrome-profile-1-0000abcd"),
            cdp_port: 61_101,
            proxy: OsString::from("http://127.0.0.1:9"),
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
}

impl Shared {
    pub(crate) fn new(launcher: Launcher) -> Shared {
        Shared {
            launcher,
            state: Mutex::new(State {
                generation: 0,
                live: None,
                in_flight: Vec::new(),
                idle_since: now(),
                restarted: false,
                closing: false,
                ended: false,
            }),
            out: Mutex::new(io::stdout()),
            closing: AtomicBool::new(false),
            oneshot: Mutex::new(None),
        }
    }
}

/// What runs now, and the requests it has not answered.
pub(crate) struct State {
    /// Moved on by every start and stop.
    pub(crate) generation: u64,
    /// Chrome and agent-browser, while they run.
    pub(crate) live: Option<Live>,
    /// The requests sent to the running agent-browser and not yet answered: each id, and
    /// whether it is a tool call.
    in_flight: Vec<(Value, bool)>,
    /// When the browser started or last answered a tool call.
    idle_since: Instant,
    /// Whether Chrome has died once already; a second death ends the guard.
    pub(crate) restarted: bool,
    /// Set once the agent has closed stdin: nothing starts or idles any more.
    pub(crate) closing: bool,
    /// Set by the relay's end, under this lock, before its final [`stop`]: from then on no
    /// request starts the browser or a one-shot agent-browser; each gets an error instead.
    pub(crate) ended: bool,
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
    pub(crate) fn sent(&mut self, id: Value, call: bool) {
        self.in_flight.push((id, call));
    }

    /// Takes the reply to `id` off the list. A tool call's reply restarts the idle time.
    pub(crate) fn answered(&mut self, id: &Value) {
        if let Some(at) = self.in_flight.iter().position(|(sent, _)| sent == id) {
            let (_, call) = self.in_flight.remove(at);
            if call {
                self.idle_since = now();
            }
        }
    }

    /// The ids still unanswered, which the guard now answers itself.
    pub(crate) fn take_in_flight(&mut self) -> Vec<Value> {
        self.in_flight.drain(..).map(|(id, _)| id).collect()
    }

    /// How long until the browser has idled for `idle`: `None` unless it runs with nothing in
    /// flight and the agent is still there.
    pub(crate) fn idle_left(&self, idle: Duration) -> Option<Duration> {
        (self.live.is_some() && self.in_flight.is_empty() && !self.closing)
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
) -> Result<(), String> {
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
) -> Result<(), String> {
    let launcher = &shared.launcher;
    wait_for_port(launcher.cdp_port, parent)?;
    let spec = Spec {
        executable: &launcher.executable,
        cwd: &launcher.cwd,
        profile: &launcher.profile,
        cdp_port: launcher.cdp_port,
        proxy: &launcher.proxy,
        env: &launcher.chrome_env,
    };
    record::write_state(&launcher.cwd, &launcher.run, &ChromeState::Starting);
    let mut chrome = chrome::launch(
        &spec,
        #[cfg(windows)]
        &launcher.job,
    )?;
    record::write_state(&launcher.cwd, &launcher.run, &chrome.recorded());
    // Before the wait for Chrome's endpoint: a guard killed during it leaves the reaper to end
    // Chrome's group. Once the guard reaps Chrome, the reaper leaves its pid alone.
    #[cfg(unix)]
    {
        launcher.reaper.say(&format!("chrome {}", chrome.pid()));
        let teller = launcher.reaper.clone();
        chrome.on_reap(move || teller.say("reaped chrome"));
    }
    note(&format!(
        "Chrome started; waiting for it to listen on the claimed debugging port {}",
        launcher.cdp_port
    ));
    let endpoint = chrome.endpoint(launcher.cdp_port, parent)?;
    note(&format!(
        "Chrome is listening on the claimed debugging port {}",
        launcher.cdp_port
    ));
    let mut env = launcher.env.clone();
    env::set_cdp(&mut env, &endpoint);
    let cannot_run =
        |error: io::Error| format!("cannot run {}: {error}", launcher.program.display());
    let mut agent = pumps::spawn_child(&launcher.program, &launcher.args, &launcher.cwd, &env)
        .map_err(cannot_run)?;
    #[cfg(windows)]
    if let Err(error) = launcher.job.adopt(&mut agent, "agent-browser") {
        drop(agent.wait());
        return Err(cannot_run(error));
    }
    #[cfg(unix)]
    launcher.reaper.say(&format!("agent {}", agent.id()));
    let (Some(stdin), Some(stdout)) = (agent.stdin.take(), agent.stdout.take()) else {
        drop(end_agent(launcher, &mut agent));
        return Err(cannot_run(io::Error::other(
            "its stdin or stdout is not piped",
        )));
    };
    state.generation += 1;
    let generation = state.generation;
    {
        let events = events.clone();
        chrome.watch(move || send(&events, Event::ChromeExited(generation)));
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

/// Returns once nothing listens on the claimed debugging port (a bind there succeeds), at most
/// [`PORT_WAIT`], looking at the guard's parent every [`parent::POLL`].
fn wait_for_port(port: u16, parent: &Parent) -> Result<(), String> {
    let polls = PORT_WAIT.as_millis() / parent::POLL.as_millis();
    for poll in 0..=polls {
        if TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok() {
            return Ok(());
        }
        if poll == 0 {
            note(&format!(
                "the claimed debugging port {port} is taken; waiting up to {} s for it",
                PORT_WAIT.as_secs()
            ));
        }
        if parent.gone() {
            return Err(PARENT_GONE.to_owned());
        }
        pause(parent::POLL);
    }
    Err(format!(
        "the claimed debugging port {port} stayed taken for {} s",
        PORT_WAIT.as_secs()
    ))
}
