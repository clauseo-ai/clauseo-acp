//! The `e2e` build's scripted sign-in (`E2E_FAKE_SIGNIN`), for the UD2 sign-in specs: the e2e
//! app's agents are `fake_agent`, which has no plan login, so a spec would stop at the sign-in
//! card. [`FakeSignin`] wraps the backend's seam over S18 ([`SetupAgents`]) and plays a script
//! for each agent the variable names; every other agent, and every call it does not script,
//! goes to S18 as usual.
//!
//! - `E2E_FAKE_SIGNIN=<agent>=<script>[,<agent>=<script>]`, `<script>` `ok` or `fail`; unset,
//!   empty or nothing valid in it leaves S18 alone.
//! - A sign-in plays `Queued`, then 200 ms later `Waiting` (its test page, [`TEST_URL`], is only
//!   logged: no browser opens in CI), then after `E2E_FAKE_SIGNIN_HOLD_MS` (default 1500)
//!   `SignedIn { email: e2e@example.invalid, source: Own }` (`ok`) or `Failed { signin.failed }`
//!   (`fail`).
//! - A cancel during the run ends it `Failed { signin.cancelled }`; a status check, a search
//!   (`FindLogin`) or a sign-out ends any run and says `SignedOut` (`none_found` for the first
//!   two); "Use it" finds nothing; "a different account" plays the sign-in.
//! - Its states reach the pump as S18's do, on a second channel merged with S18's, which ends
//!   when S18's does ([`wrap`]).
//!
//! Contract: built only with the `e2e` feature (`setup.rs` declares the module under it, and
//! `start.rs` calls [`wrap`] under it), so no release reads the variables or holds a script.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_agents::{
    AgentsEvent, Clock, Found, InstallState, Installed, Method, PasteCode, SigninCode, SigninError,
    SigninState, Source,
};
use clauseo_protocol::AgentId;
use futures::channel::mpsc;
use futures::future::ready;
use futures::stream::{self, BoxStream, Stream, StreamExt};

use super::backend::SetupAgents;
use crate::bridge::lock;

/// The agents and their scripts.
const SCRIPTS_VAR: &str = "E2E_FAKE_SIGNIN";
/// How long `Waiting` lasts, in ms.
const HOLD_VAR: &str = "E2E_FAKE_SIGNIN_HOLD_MS";
/// How long `Queued` lasts.
const QUEUED_FOR: Duration = Duration::from_millis(200);
/// How long `Waiting` lasts unless `E2E_FAKE_SIGNIN_HOLD_MS` says.
const HOLD: Duration = Duration::from_millis(1500);
/// The account an `ok` sign-in ends with.
pub(crate) const EMAIL: &str = "e2e@example.invalid";
/// The page the script "opens": logged only.
pub(crate) const TEST_URL: &str = "https://example.invalid/e2e-signin";

/// How a scripted sign-in ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Script {
    /// `SignedIn`.
    Ok,
    /// `Failed { signin.failed }`.
    Fail,
}

/// The scripts in `value` (`E2E_FAKE_SIGNIN`'s form); an entry that is no `<agent>=ok|fail`
/// is dropped.
pub(crate) fn scripts_from(value: Option<&str>) -> BTreeMap<AgentId, Script> {
    value
        .unwrap_or_default()
        .split(',')
        .filter_map(|entry| {
            let (agent, script) = entry.split_once('=')?;
            let script = match script.trim() {
                "ok" => Script::Ok,
                "fail" => Script::Fail,
                _ => return None,
            };
            Some((AgentId::new(agent.trim()).ok()?, script))
        })
        .collect()
}

/// `Waiting`'s length from `E2E_FAKE_SIGNIN_HOLD_MS`'s `value`; [`HOLD`] when unset or no number.
fn hold_from(value: Option<&str>) -> Duration {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map_or(HOLD, Duration::from_millis)
}

/// The steps' lengths.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Steps {
    /// How long `Queued` lasts.
    pub(crate) queued_for: Duration,
    /// How long `Waiting` lasts.
    pub(crate) hold: Duration,
}

/// The seam over S18 with `E2E_FAKE_SIGNIN`'s scripts, and the pump's events: S18's, merged
/// with the scripts'. With no script, `real` and `source` as they are.
pub(crate) fn wrap(
    real: Arc<dyn SetupAgents>,
    source: impl Stream<Item = AgentsEvent> + Send + 'static,
    clock: Arc<dyn Clock>,
) -> (Arc<dyn SetupAgents>, BoxStream<'static, AgentsEvent>) {
    let scripts = scripts_from(std::env::var(SCRIPTS_VAR).ok().as_deref());
    if scripts.is_empty() {
        return (real, source.boxed());
    }
    tracing::info!(
        agents = scripts.len(),
        "e2e: scripted sign-ins stand in for S18's"
    );
    let steps = Steps {
        queued_for: QUEUED_FOR,
        hold: hold_from(std::env::var(HOLD_VAR).ok().as_deref()),
    };
    let (fake, events) = FakeSignin::new(real, scripts, clock, steps);
    (Arc::new(fake), merged(source, events))
}

/// `primary`'s events and `fake`'s as they come, ending when `primary` ends.
pub(crate) fn merged(
    primary: impl Stream<Item = AgentsEvent> + Send + 'static,
    fake: mpsc::UnboundedReceiver<AgentsEvent>,
) -> BoxStream<'static, AgentsEvent> {
    let primary = primary.map(Some).chain(stream::once(ready(None)));
    stream::select(primary, fake.map(Some))
        .take_while(|event| ready(event.is_some()))
        .filter_map(ready)
        .boxed()
}

/// One scripted agent's latest run.
#[derive(Default)]
struct Run {
    /// Counts the runs: a run whose number is not the latest says nothing more.
    generation: u64,
    /// The latest state said.
    state: Option<SigninState>,
    /// Whether the run is still going.
    going: bool,
}

/// What a run's thread shares with the seam.
struct Shared {
    clock: Arc<dyn Clock>,
    events: mpsc::UnboundedSender<AgentsEvent>,
    runs: Mutex<BTreeMap<AgentId, Run>>,
}

impl Shared {
    /// A new run for `agent`, ending any before it: its number.
    fn begin(&self, agent: &AgentId) -> u64 {
        let mut runs = lock(&self.runs);
        let run = runs.entry(agent.clone()).or_default();
        run.generation += 1;
        run.going = false;
        run.generation
    }

    /// Says `state` for run `generation` of `agent`, unless a newer run began: under the lock,
    /// so states go out in the order they are said. Whether it was said.
    fn say(&self, agent: &AgentId, generation: u64, state: SigninState, going: bool) -> bool {
        let mut runs = lock(&self.runs);
        let Some(run) = runs
            .get_mut(agent)
            .filter(|run| run.generation == generation)
        else {
            return false;
        };
        run.going = going;
        run.state = Some(state.clone());
        // Fails only once the pump is gone: nobody listens then.
        drop(self.events.unbounded_send(AgentsEvent::Signin {
            agent: agent.clone(),
            state,
        }));
        true
    }

    /// A new run that says `state` at once and ends.
    fn end_with(&self, agent: &AgentId, state: SigninState) {
        let generation = self.begin(agent);
        self.say(agent, generation, state, false);
    }

    fn failed(&self, code: SigninCode) -> SigninState {
        SigninState::Failed {
            code,
            at: self.clock.now(),
        }
    }
}

/// Plays `script`'s steps after `Queued`, unless a newer run begins.
async fn play(shared: &Shared, agent: &AgentId, generation: u64, script: Script, steps: Steps) {
    async_io::Timer::after(steps.queued_for).await;
    let waiting = SigninState::Waiting {
        since: shared.clock.now(),
        nudge: false,
    };
    if !shared.say(agent, generation, waiting, true) {
        return;
    }
    tracing::info!(%agent, url = TEST_URL, "e2e: the scripted sign-in waits on its test page");
    async_io::Timer::after(steps.hold).await;
    let end = match script {
        Script::Ok => SigninState::SignedIn {
            email: Some(EMAIL.to_owned()),
            plan: None,
            source: Source::Own,
            at: shared.clock.now(),
        },
        Script::Fail => shared.failed(SigninCode::Failed),
    };
    shared.say(agent, generation, end, false);
}

/// The seam with scripts for some agents.
pub(crate) struct FakeSignin {
    real: Arc<dyn SetupAgents>,
    scripts: BTreeMap<AgentId, Script>,
    shared: Arc<Shared>,
    steps: Steps,
}

impl FakeSignin {
    /// The seam over `real` with `scripts`, and the scripts' events.
    pub(crate) fn new(
        real: Arc<dyn SetupAgents>,
        scripts: BTreeMap<AgentId, Script>,
        clock: Arc<dyn Clock>,
        steps: Steps,
    ) -> (Self, mpsc::UnboundedReceiver<AgentsEvent>) {
        let (events, receiver) = mpsc::unbounded();
        let shared = Arc::new(Shared {
            clock,
            events,
            runs: Mutex::new(BTreeMap::new()),
        });
        let fake = Self {
            real,
            scripts,
            shared,
            steps,
        };
        (fake, receiver)
    }

    /// `agent`'s script, when it has one.
    fn script(&self, agent: &AgentId) -> Option<Script> {
        self.scripts.get(agent).copied()
    }

    /// Starts `script`'s run for `agent`: `Queued` now, the rest on a thread of its own.
    fn start(&self, agent: &AgentId, script: Script) {
        let generation = self.shared.begin(agent);
        self.shared
            .say(agent, generation, SigninState::Queued, true);
        let shared = Arc::clone(&self.shared);
        let who = agent.clone();
        let steps = self.steps;
        let spawned = std::thread::Builder::new()
            .name("clauseo-e2e-signin".to_owned())
            .spawn(move || {
                futures::executor::block_on(play(&shared, &who, generation, script, steps));
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "e2e: the scripted sign-in did not start");
            let failed = self.shared.failed(SigninCode::Failed);
            self.shared.say(agent, generation, failed, false);
        }
    }
}

impl SetupAgents for FakeSignin {
    fn install(&self, agent: &AgentId) {
        self.real.install(agent);
    }
    fn retry(&self, agent: &AgentId, use_system_proxy: bool) {
        self.real.retry(agent, use_system_proxy);
    }
    fn state(&self, agent: &AgentId) -> Option<InstallState> {
        self.real.state(agent)
    }
    fn installed(&self, agent: &AgentId) -> Option<Installed> {
        self.real.installed(agent)
    }
    fn sign_in(&self, agent: &AgentId, method: Method) -> Result<(), SigninError> {
        match self.script(agent) {
            Some(script) => {
                self.start(agent, script);
                Ok(())
            }
            None => self.real.sign_in(agent, method),
        }
    }
    fn cancel_sign_in(&self, agent: &AgentId) -> Result<(), SigninError> {
        if self.script(agent).is_none() {
            return self.real.cancel_sign_in(agent);
        }
        if !self.sign_in_going(agent) {
            return Err(SigninError::NotRunning(agent.clone()));
        }
        let cancelled = self.shared.failed(SigninCode::Cancelled);
        self.shared.end_with(agent, cancelled);
        Ok(())
    }
    fn paste_code(&self, agent: &AgentId, code: PasteCode) -> Result<(), SigninError> {
        match self.script(agent) {
            Some(_) => Err(SigninError::NotRunning(agent.clone())),
            None => self.real.paste_code(agent, code),
        }
    }
    fn sign_out(&self, agent: &AgentId) -> Result<(), SigninError> {
        if self.script(agent).is_none() {
            return self.real.sign_out(agent);
        }
        self.shared
            .end_with(agent, SigninState::SignedOut { found: None });
        Ok(())
    }
    fn check_status(&self, agent: &AgentId) -> Result<(), SigninError> {
        if self.script(agent).is_none() {
            return self.real.check_status(agent);
        }
        let none = SigninState::SignedOut {
            found: Some(Found::NoLogin),
        };
        self.shared.end_with(agent, none);
        Ok(())
    }
    fn find_login(&self, agent: &AgentId) -> Result<(), SigninError> {
        if self.script(agent).is_none() {
            return self.real.find_login(agent);
        }
        self.check_status(agent)
    }
    fn use_found_login(&self, agent: &AgentId) -> Result<(), SigninError> {
        match self.script(agent) {
            Some(_) => Err(SigninError::NothingFound(agent.clone())),
            None => self.real.use_found_login(agent),
        }
    }
    fn use_different_account(&self, agent: &AgentId) -> Result<(), SigninError> {
        match self.script(agent) {
            Some(script) => {
                self.start(agent, script);
                Ok(())
            }
            None => self.real.use_different_account(agent),
        }
    }
    fn signin_state(&self, agent: &AgentId) -> Option<SigninState> {
        if self.script(agent).is_none() {
            return self.real.signin_state(agent);
        }
        lock(&self.shared.runs)
            .get(agent)
            .and_then(|run| run.state.clone())
    }
    fn sign_in_going(&self, agent: &AgentId) -> bool {
        if self.script(agent).is_none() {
            return self.real.sign_in_going(agent);
        }
        lock(&self.shared.runs)
            .get(agent)
            .is_some_and(|run| run.going)
    }
    fn stop_using_login(&self, agent: &AgentId, why: Option<Found>) -> Result<(), SigninError> {
        match self.script(agent) {
            Some(_) => Err(SigninError::NotReused(agent.clone())),
            None => self.real.stop_using_login(agent, why),
        }
    }
    fn install_tries(&self, agent: &AgentId) -> Option<u32> {
        self.real.install_tries(agent)
    }
    fn shutdown(&self, wait: Duration) -> bool {
        self.real.shutdown(wait)
    }
}

#[cfg(test)]
mod tests;
