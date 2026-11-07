//! The start's `Ready` from a receipt, sent at once, with the Node check on its own thread: a
//! slow or hung `node --version` delays neither the first `Ready` nor the `Agents` commands, and
//! a check that fails turns the agent `Failed`, which `retry` repairs. The launcher holds or
//! fails each `node --version` as the test says. Events are read under a bound (a thread hands
//! them to a channel with `recv_timeout`), so a start that waits for the check fails the test
//! rather than stalling it. `receipt_signin.rs` uses the same launcher and channel.

use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::time::Duration;

use clauseo_agents::{Agents, AgentsEvent, FailureKind, InstallState, Installed, SigninState};
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_protocol::AgentId;

use crate::support::{Fixture, agent_a, agent_b};

/// The bound on every wait for an event.
pub(crate) const BOUND: Duration = Duration::from_secs(20);

/// What the launcher does with a `node --version`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Runs it.
    Pass,
    /// Holds it until the mode changes.
    Hold,
    /// Fails it: the launch is of a program that does not exist.
    Fail,
}

#[derive(Debug)]
struct State {
    mode: Mode,
    /// `node --version` launches held so far.
    held: usize,
    /// Every other launch fails too: the receipt's Node cannot start.
    fail_runs: bool,
}

/// The test's hold on each `node --version`.
#[derive(Debug)]
pub(crate) struct Gate {
    state: Mutex<State>,
    changed: Condvar,
}

impl Gate {
    pub(crate) fn new(mode: Mode) -> Arc<Gate> {
        Arc::new(Gate {
            state: Mutex::new(State {
                mode,
                held: 0,
                fail_runs: false,
            }),
            changed: Condvar::new(),
        })
    }

    pub(crate) fn fail_runs(&self) {
        self.lock().fail_runs = true;
    }

    pub(crate) fn set(&self, mode: Mode) {
        self.lock().mode = mode;
        self.changed.notify_all();
    }

    pub(crate) fn held(&self) -> usize {
        self.lock().held
    }

    /// Blocks, at most [`BOUND`], until a `node --version` is held.
    pub(crate) fn wait_held(&self) {
        let (state, _) = self
            .changed
            .wait_timeout_while(self.lock(), BOUND, |state| state.held == 0)
            .unwrap_or_else(PoisonError::into_inner);
        assert!(state.held > 0, "no node --version was held");
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The passthrough launcher, with each `node --version` passed through the gate.
struct Gated {
    inner: PassthroughLauncher,
    gate: Arc<Gate>,
}

impl Launcher for Gated {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        if spec.args() != ["--version"] {
            if self.gate.lock().fail_runs {
                return self.inner.launch(&missing(spec)?);
            }
            return self.inner.launch(spec);
        }
        let mut state = self.gate.lock();
        if state.mode == Mode::Hold {
            state.held += 1;
            self.gate.changed.notify_all();
            while state.mode == Mode::Hold {
                state = self
                    .gate
                    .changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        }
        let mode = state.mode;
        drop(state);
        if mode == Mode::Fail {
            return self.inner.launch(&missing(spec)?);
        }
        self.inner.launch(spec)
    }
}

/// `spec` with a program that does not exist.
fn missing(spec: &LaunchSpec) -> Result<LaunchSpec, clauseo_process::Error> {
    LaunchSpec::new(
        spec.cwd().join("no-such-node"),
        spec.args().to_vec(),
        spec.env().clone(),
        spec.cwd().to_owned(),
    )
}

/// Installs agent A on the fixture with the real Node, and stops.
fn install_a(fx: &Fixture) -> Installed {
    install_first(fx)
}

/// Installs the fixture's first agent (A, under whatever id it has) with the real Node, and
/// stops.
pub(crate) fn install_first(fx: &Fixture) -> Installed {
    let (agents, events) = fx.start();
    events.next();
    events.next();
    let first = fx.config.snapshot.agents().next().unwrap().id.clone();
    agents.install(&first);
    let (_, last) = events.until_end(&first);
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    agents.installed(&first).unwrap()
}

/// The install and sign-in events, handed over by a thread that ends once the events close.
pub(crate) struct Events {
    install: mpsc::Receiver<(AgentId, InstallState)>,
    pub(crate) signin: mpsc::Receiver<(AgentId, SigninState)>,
}

/// Starts `Agents` on the fixture with the gated launcher.
pub(crate) fn start_gated(fx: &Fixture, gate: &Arc<Gate>) -> (Agents, Events) {
    let launcher: Arc<dyn Launcher> = Arc::new(Gated {
        inner: PassthroughLauncher::new(SpawnOptions::new()),
        gate: Arc::clone(gate),
    });
    let (agents, events) = Agents::start(fx.config.clone(), launcher, fx.timer.clone());
    let (install, install_rx) = mpsc::channel();
    let (signin, signin_rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(event) = futures::executor::block_on(events.recv()) {
            // A send fails only once the test dropped its receiver.
            match event {
                AgentsEvent::Install { agent, state } => drop(install.send((agent, state))),
                AgentsEvent::Signin { agent, state } => drop(signin.send((agent, state))),
                _ => {}
            }
        }
    });
    let events = Events {
        install: install_rx,
        signin: signin_rx,
    };
    (agents, events)
}

/// The next install event, or `None` once [`BOUND`] passes with none.
pub(crate) fn next_within(events: &Events) -> Option<(AgentId, InstallState)> {
    events.install.recv_timeout(BOUND).ok()
}

/// Install events up to `agent`'s first one for which `done` holds, under [`BOUND`] each.
pub(crate) fn until_within(
    events: &Events,
    agent: &AgentId,
    done: impl Fn(&InstallState) -> bool,
) -> InstallState {
    loop {
        let (id, state) = next_within(events).expect("an event within the bound");
        if id == *agent && done(&state) {
            return state;
        }
    }
}

#[test]
fn an_installed_agent_is_ready_before_a_slow_node_check_ends() {
    let fx = Fixture::new("receipt-slow");
    let installed = install_a(&fx);
    let gate = Gate::new(Mode::Hold);
    let (agents, events) = start_gated(&fx, &gate);
    gate.wait_held();

    let first = next_within(&events);
    assert_eq!(
        first,
        Some((
            agent_a(),
            InstallState::Ready {
                version: installed.version.clone()
            }
        )),
        "Ready came before the check ended"
    );
    assert_eq!(gate.held(), 1, "the check is still held");
    assert_eq!(agents.installed(&agent_a()), Some(installed.clone()));

    gate.set(Mode::Pass);
    let b = until_within(&events, &agent_b(), |_| true);
    assert!(matches!(b, InstallState::Missing { .. }), "{b:?}");
    assert!(agents.shutdown(BOUND), "the check's thread ended");
    // The check found the receipt's Node good: A stays as it was sent.
    assert_eq!(agents.installed(&agent_a()), Some(installed));
}

#[test]
fn a_launch_during_a_hung_node_check_goes_ahead_at_once() {
    let fx = Fixture::new("receipt-hung");
    let installed = install_a(&fx);
    let gate = Gate::new(Mode::Hold);
    let (agents, events) = start_gated(&fx, &gate);
    gate.wait_held();
    let first = until_within(&events, &agent_a(), |_| true);
    assert!(matches!(first, InstallState::Ready { .. }), "{first:?}");

    // What a launch builds from is there now, the receipt's install, while the check hangs.
    assert_eq!(agents.installed(&agent_a()), Some(installed));
    // And the worker answers commands: `install` on a `Ready` agent sends `Ready` again.
    agents.install(&agent_a());
    let again = until_within(&events, &agent_a(), |_| true);
    assert!(matches!(again, InstallState::Ready { .. }), "{again:?}");
    assert_eq!(gate.held(), 1, "the check is still held");

    gate.set(Mode::Pass);
    assert!(agents.shutdown(BOUND), "the check's thread ended");
}

#[test]
fn a_failed_node_check_turns_the_agent_failed_and_retry_repairs_it() {
    let fx = Fixture::new("receipt-failed");
    let installed = install_a(&fx);
    let gate = Gate::new(Mode::Fail);
    let (agents, events) = start_gated(&fx, &gate);
    let first = until_within(&events, &agent_a(), |_| true);
    assert!(matches!(first, InstallState::Ready { .. }), "{first:?}");
    let failed = until_within(&events, &agent_a(), |_| true);
    let InstallState::Failed(failure) = failed else {
        panic!("{failed:?}");
    };
    assert_eq!(failure.kind, FailureKind::SetupFailed);
    assert_eq!(
        failure.log.lines().last().map(String::as_str),
        Some("result: setup_failed after 0 tries")
    );
    assert!(
        failure
            .log
            .lines()
            .iter()
            .any(|line| line == "the Node this agent runs on did not pass its check at start"),
        "{:?}",
        failure.log.lines()
    );

    gate.set(Mode::Pass);
    agents.retry(&agent_a(), false);
    let last = until_within(&events, &agent_a(), |state| {
        matches!(state, InstallState::Ready { .. } | InstallState::Failed(_))
    });
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(agents.installed(&agent_a()), Some(installed));
    assert_eq!(agents.install_tries(&agent_a()), Some(0));
}

#[test]
fn a_launch_after_a_failed_node_check_gets_the_install_setup_failed_error() {
    let fx = Fixture::new("receipt-launch-failed");
    install_a(&fx);
    let gate = Gate::new(Mode::Fail);
    let (agents, events) = start_gated(&fx, &gate);
    until_within(&events, &agent_a(), |state| {
        matches!(state, InstallState::Failed(_))
    });

    // No install to launch from, and the state's code says why: never a wait.
    assert_eq!(agents.installed(&agent_a()), None);
    let Some(InstallState::Failed(failure)) = agents.state(&agent_a()) else {
        panic!("{:?}", agents.state(&agent_a()));
    };
    assert_eq!(failure.kind.code(), "agent.install_setup_failed");
}
