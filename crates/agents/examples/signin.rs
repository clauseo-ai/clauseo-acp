//! Drives S18's sign-in engine by hand against the real Claude Code or Codex CLI, for the G7
//! gate rows. Not part of any automatic gate.
//!
//! `cargo run -p clauseo-agents --example signin -- <claude-acp|codex-acp> <command>`
//!
//! The agent's ACP adapter is installed first, exactly as `install` does, and then one command
//! runs through `Agents`:
//! - `status`: `check_status`, the start check. It may report `LoginFound` when the lawyer
//!   already has a login of their own.
//! - `claudeai` or `console` (Claude), `browser` or `device` (Codex): `sign_in` with that method.
//! - `use-it`: `use_found_login`, after a `LoginFound`. `different`: `use_different_account`.
//!   `stop`: `stop_using_login` with no reason, which writes `no-reuse`.
//! - `sign-out`: `sign_out`, Clauseo's own login only.
//!
//! **`stop`, `different` and `sign-out` act on real logins.** `stop` and `different` change
//! which login Clauseo uses from now on; `sign-out` runs the vendor's logout on Clauseo's own
//! login. The engine refuses `sign-out` while the source is the lawyer's own reused login, and
//! Clauseo never signs out the lawyer's login. Read each step's output before the next command.
//!
//! Every `AgentsEvent` for the agent prints one line, with the seconds since start. A sign-in's
//! `OpenUrl` prints `open this address: <url>`; nothing opens a browser, so click it yourself.
//! While the engine waits, each line on stdin is read as input: `cancel` cancels the sign-in,
//! `again` asks Codex to try its port again, and any other non-empty line is pasted to Claude's
//! login as the code. A pasted code is never printed. The run ends when the state settles
//! (`SignedIn`, `SignedOut`, `LoginFound` or `Failed`), printing `exit code 0`, or `exit code 1`
//! on `Failed`. A call the engine refuses prints why and exits 1. With no new event for 60 s,
//! the current state prints again, and a settled one ends the run.
//!
//! The data folder is the app's (`CLAUSEO_DATA_DIR` replaces it), and the agents' homes are
//! `Locations::agent_home`. The base environment is this process's, and the children get only
//! what `signin/env.rs` builds: the launcher clears the rest, as 0046 requires.

use std::error::Error;
use std::io::{self, BufRead, Write};
use std::pin::pin;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clauseo_agents::{
    Agents, AgentsConfig, AgentsEvent, Clock, InstallState, Method, PasteCode, Platform,
    SigninError, SigninState, SystemClock,
};
use clauseo_process::{AsyncIoTimer, Launcher, PassthroughLauncher, SpawnOptions, Timer};
use clauseo_protocol::AgentId;
use clauseo_settings::{Locations, SystemDirs, SystemEnv};
use futures::future::{Either, select};
use zeroize::Zeroizing;

/// The two agents this example signs in.
const AGENTS: [&str; 2] = ["claude-acp", "codex-acp"];

/// How long the events stay silent before the state prints again.
const QUIET: Duration = Duration::from_secs(60);

const USAGE: &str = "usage: signin <claude-acp|codex-acp> \
    <status|claudeai|console|browser|device|use-it|different|stop|sign-out>";

type Fallible<T> = Result<T, Box<dyn Error>>;

/// One command, as the engine's call it makes.
#[derive(Debug, Clone, Copy)]
enum Action {
    Status,
    SignIn(Method),
    UseIt,
    Different,
    Stop,
    SignOut,
}

/// What the event loop waits for next.
enum Wake {
    /// An event from the engine; `None` once the worker is gone.
    Event(Option<AgentsEvent>),
    /// A line from stdin; `None` once stdin is closed.
    Line(Option<Zeroizing<String>>),
    /// No event for [`QUIET`].
    Quiet,
}

/// The agent's events and stdin, from the first install call on.
struct Session<'a> {
    agents: &'a Agents,
    agent: AgentId,
    events: async_channel::Receiver<AgentsEvent>,
    lines: async_channel::Receiver<Zeroizing<String>>,
    lines_open: bool,
    started: Duration,
}

fn main() -> ExitCode {
    let mut out = io::stdout().lock();
    match run(&mut out) {
        Ok(code) => code,
        Err(error) => {
            // Nothing is left to do when stderr is gone too.
            drop(writeln!(io::stderr().lock(), "error: {error}"));
            ExitCode::FAILURE
        }
    }
}

fn run(out: &mut impl Write) -> Fallible<ExitCode> {
    let clock = SystemClock;
    let started = clock.monotonic();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (agent, action) = parse(&args)?;
    let locations = Locations::resolve(&SystemDirs, &SystemEnv)?;
    let platform = Platform::current().ok_or("this platform has no Node download")?;
    let mut config = AgentsConfig::new(locations.data_dir().to_owned(), platform);
    config.install.proxy = std::env::var("HTTPS_PROXY")
        .or_else(|_unset| std::env::var("https_proxy"))
        .into_iter()
        .find(|proxy| !proxy.is_empty());
    for name in AGENTS {
        let id = AgentId::new(name).map_err(|error| format!("{name}: {error}"))?;
        config.signin.homes.insert(id, locations.agent_home(name));
    }
    writeln!(out, "data folder: {}", locations.data_dir().display())?;

    let launcher: Arc<dyn Launcher> =
        Arc::new(PassthroughLauncher::new(SpawnOptions::new().clear_env()));
    let (agents, events) = Agents::start(config, launcher, AsyncIoTimer);
    agents.install(&agent);
    let mut session = Session {
        agents: &agents,
        agent: agent.clone(),
        events,
        lines: read_lines(),
        lines_open: true,
        started,
    };
    if !futures::executor::block_on(session.install(out))? {
        writeln!(out, "install did not finish, so no sign-in ran")?;
        writeln!(out, "exit code 1")?;
        return Ok(ExitCode::FAILURE);
    }

    if let Err(error) = call(&agents, &agent, action) {
        writeln!(out, "refused: {error}")?;
        writeln!(out, "exit code 1")?;
        return Ok(ExitCode::FAILURE);
    }
    let ok = futures::executor::block_on(session.settle(out))?;
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// The agent and the command the arguments name. A method that is the other agent's is refused
/// here, before the install.
fn parse(args: &[String]) -> Result<(AgentId, Action), String> {
    let [id, word] = args else {
        return Err(USAGE.to_owned());
    };
    if !AGENTS.contains(&id.as_str()) {
        return Err(format!(
            "{id} is not an agent this example signs in\n{USAGE}"
        ));
    }
    let action = match (id.as_str(), word.as_str()) {
        (_, "status") => Action::Status,
        (_, "use-it") => Action::UseIt,
        (_, "different") => Action::Different,
        (_, "stop") => Action::Stop,
        (_, "sign-out") => Action::SignOut,
        ("claude-acp", "claudeai") => Action::SignIn(Method::ClaudeAi),
        ("claude-acp", "console") => Action::SignIn(Method::Console),
        ("codex-acp", "browser") => Action::SignIn(Method::Browser),
        ("codex-acp", "device") => Action::SignIn(Method::DeviceCode),
        _ => return Err(format!("{word} is not a command for {id}\n{USAGE}")),
    };
    let agent = AgentId::new(id).map_err(|error| format!("{id}: {error}"))?;
    Ok((agent, action))
}

/// The engine call for `action`.
fn call(agents: &Agents, agent: &AgentId, action: Action) -> Result<(), SigninError> {
    match action {
        Action::Status => agents.check_status(agent),
        Action::SignIn(method) => agents.sign_in(agent, method),
        Action::UseIt => agents.use_found_login(agent),
        Action::Different => agents.use_different_account(agent),
        Action::Stop => agents.stop_using_login(agent, None),
        Action::SignOut => agents.sign_out(agent),
    }
}

/// Stdin, one line at a time, on a thread of its own: the event loop must not block on it.
/// The thread ends with stdin, or with the process.
fn read_lines() -> async_channel::Receiver<Zeroizing<String>> {
    let (sender, receiver) = async_channel::unbounded();
    let reader = std::thread::Builder::new()
        .name("signin-stdin".to_owned())
        .spawn(move || {
            loop {
                let mut line = Zeroizing::new(String::new());
                match io::stdin().lock().read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if sender.try_send(line).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    // With no reader the sender is gone with it, so the receiver reports closed.
    drop(reader);
    receiver
}

/// Whether a sign-in state has settled, and whether it is a success.
fn settled(state: &SigninState) -> Option<bool> {
    match state {
        SigninState::SignedIn { .. }
        | SigninState::SignedOut { .. }
        | SigninState::LoginFound { .. } => Some(true),
        SigninState::Failed { .. } => Some(false),
        _ => None,
    }
}

/// The agent an event is about, for the events this example prints.
fn about(event: &AgentsEvent) -> Option<&AgentId> {
    match event {
        AgentsEvent::Install { agent, .. }
        | AgentsEvent::Signin { agent, .. }
        | AgentsEvent::OpenUrl { agent, .. }
        | AgentsEvent::SigninNote { agent, .. } => Some(agent),
        _ => None,
    }
}

impl Session<'_> {
    /// The next thing to handle: an event, a line from stdin, or a quiet minute.
    async fn next(&self) -> Wake {
        let event = async { Wake::Event(self.events.recv().await.ok()) };
        let line = async {
            if self.lines_open {
                Wake::Line(self.lines.recv().await.ok())
            } else {
                futures::future::pending().await
            }
        };
        let either = async {
            match select(pin!(event), pin!(line)).await {
                Either::Left((wake, _)) | Either::Right((wake, _)) => wake,
            }
        };
        let quiet = async {
            AsyncIoTimer.after(QUIET).await;
            Wake::Quiet
        };
        match select(pin!(either), pin!(quiet)).await {
            Either::Left((wake, _)) | Either::Right((wake, _)) => wake,
        }
    }

    /// Seconds since the run started, for the printed lines.
    fn elapsed(&self) -> f64 {
        SystemClock
            .monotonic()
            .saturating_sub(self.started)
            .as_secs_f64()
    }

    /// Prints one event for the agent, one line.
    fn show(&self, out: &mut impl Write, event: &AgentsEvent) -> io::Result<()> {
        let at = self.elapsed();
        match event {
            AgentsEvent::Install { state, .. } => writeln!(out, "{at:>7.1} s install: {state:?}"),
            AgentsEvent::Signin { state, .. } => writeln!(out, "{at:>7.1} s signin: {state:?}"),
            AgentsEvent::OpenUrl { url, .. } => {
                writeln!(out, "{at:>7.1} s open this address: {url}")
            }
            AgentsEvent::SigninNote { note, .. } => {
                writeln!(out, "{at:>7.1} s note: {}", note.code())
            }
            _ => Ok(()),
        }
    }

    /// Waits until the agent's install is `Ready` (`true`), or stops for good (`false`).
    async fn install(&mut self, out: &mut impl Write) -> Fallible<bool> {
        loop {
            match self.next().await {
                Wake::Event(None) => return Err("the agents worker stopped".into()),
                Wake::Event(Some(event)) => {
                    if about(&event) != Some(&self.agent) {
                        continue;
                    }
                    self.show(out, &event)?;
                    if let AgentsEvent::Install { state, .. } = &event {
                        match state {
                            InstallState::Ready { .. } => return Ok(true),
                            InstallState::Failed(_)
                            | InstallState::Paused
                            | InstallState::Managed => return Ok(false),
                            _ => {}
                        }
                    }
                }
                Wake::Line(line) => self.line(out, line)?,
                Wake::Quiet => {}
            }
        }
    }

    /// Waits until the sign-in settles: `true` on a success, `false` on `Failed`. Prints the
    /// exit code last.
    async fn settle(&mut self, out: &mut impl Write) -> Fallible<bool> {
        loop {
            match self.next().await {
                Wake::Event(None) => return Err("the agents worker stopped".into()),
                Wake::Event(Some(event)) => {
                    if about(&event) != Some(&self.agent) {
                        continue;
                    }
                    self.show(out, &event)?;
                    if let AgentsEvent::Signin { state, .. } = &event
                        && let Some(ok) = settled(state)
                    {
                        return Ok(exit(out, ok)?);
                    }
                }
                Wake::Line(line) => self.line(out, line)?,
                Wake::Quiet => {
                    let state = self.agents.signin_state(&self.agent);
                    if let Some(ok) = state.as_ref().and_then(settled) {
                        return Ok(exit(out, ok)?);
                    }
                    let at = self.elapsed();
                    writeln!(out, "{at:>7.1} s no new event for 60 s; state: {state:?}")?;
                }
            }
        }
    }

    /// Handles one stdin line: `cancel`, `again`, or a code for Claude's login. A refusal is
    /// printed; the code itself never is.
    fn line(&mut self, out: &mut impl Write, line: Option<Zeroizing<String>>) -> io::Result<()> {
        let Some(line) = line else {
            self.lines_open = false;
            return Ok(());
        };
        let (sent, result) = match line.trim() {
            "" => return Ok(()),
            "cancel" => ("cancel", self.agents.cancel_sign_in(&self.agent)),
            "again" => ("try again", self.agents.try_again(&self.agent)),
            _ => (
                "code",
                self.agents.paste_code(&self.agent, PasteCode::new(line)),
            ),
        };
        let at = self.elapsed();
        match result {
            Ok(()) => writeln!(out, "{at:>7.1} s {sent} sent"),
            Err(error) => writeln!(out, "{at:>7.1} s {sent} refused: {error}"),
        }
    }
}

/// Prints the exit code for a settled run and gives back whether it succeeded.
fn exit(out: &mut impl Write, ok: bool) -> io::Result<bool> {
    writeln!(out, "exit code {}", u8::from(!ok))?;
    Ok(ok)
}
