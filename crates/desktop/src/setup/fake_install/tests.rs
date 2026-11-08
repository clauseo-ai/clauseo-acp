//! The scripted install: the variable's form, `Missing` first, the states in order to `Ready`,
//! a second press joining, `Ready` said again, S18's own install states dropped, and the calls
//! it leaves to S18.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_agents::{
    AgentsEvent, Found, InstallState, Installed, Method, PasteCode, SigninError, SigninState,
};
use clauseo_protocol::AgentId;
use futures::StreamExt;
use futures::channel::mpsc;

use super::{DOWNLOAD_BYTES, FakeInstall, VERSION, agents_from};
use crate::setup::backend::SetupAgents;

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

/// S18 stand-in: records the calls it gets.
#[derive(Default)]
struct Real {
    calls: Mutex<Vec<&'static str>>,
}

impl Real {
    fn call(&self, name: &'static str) {
        self.calls.lock().unwrap().push(name);
    }
    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }
}

impl SetupAgents for Real {
    fn install(&self, _: &AgentId) {
        self.call("install");
    }
    fn retry(&self, _: &AgentId, _: bool) {
        self.call("retry");
    }
    fn state(&self, _: &AgentId) -> Option<InstallState> {
        self.call("state");
        None
    }
    fn installed(&self, _: &AgentId) -> Option<Installed> {
        self.call("installed");
        None
    }
    fn sign_in(&self, _: &AgentId, _: Method) -> Result<(), SigninError> {
        self.call("sign_in");
        Ok(())
    }
    fn cancel_sign_in(&self, _: &AgentId) -> Result<(), SigninError> {
        Ok(())
    }
    fn paste_code(&self, _: &AgentId, _: PasteCode) -> Result<(), SigninError> {
        Ok(())
    }
    fn sign_out(&self, _: &AgentId) -> Result<(), SigninError> {
        Ok(())
    }
    fn check_status(&self, _: &AgentId) -> Result<(), SigninError> {
        Ok(())
    }
    fn find_login(&self, _: &AgentId) -> Result<(), SigninError> {
        Ok(())
    }
    fn use_found_login(&self, _: &AgentId) -> Result<(), SigninError> {
        Ok(())
    }
    fn use_different_account(&self, _: &AgentId) -> Result<(), SigninError> {
        Ok(())
    }
    fn signin_state(&self, _: &AgentId) -> Option<SigninState> {
        None
    }
    fn sign_in_going(&self, _: &AgentId) -> bool {
        false
    }
    fn stop_using_login(&self, _: &AgentId, _: Option<Found>) -> Result<(), SigninError> {
        Ok(())
    }
    fn install_tries(&self, _: &AgentId) -> Option<u32> {
        None
    }
    fn shutdown(&self, _: Duration) -> bool {
        true
    }
    fn set_network(&self, _: clauseo_agents::Network) {}
}

fn fake(hold: Duration) -> (FakeInstall, Arc<Real>, mpsc::UnboundedReceiver<AgentsEvent>) {
    let real = Arc::new(Real::default());
    let agents = BTreeSet::from([claude()]);
    let (fake, events) = FakeInstall::new(
        Arc::clone(&real) as Arc<dyn SetupAgents>,
        &agents,
        &BTreeSet::new(),
        hold,
    );
    (fake, real, events)
}

/// The next `n` install states, waiting for each.
fn states(events: &mut mpsc::UnboundedReceiver<AgentsEvent>, n: usize) -> Vec<InstallState> {
    futures::executor::block_on(events.take(n).collect::<Vec<_>>())
        .into_iter()
        .map(|event| match event {
            AgentsEvent::Install { agent, state } => {
                assert_eq!(agent, claude());
                state
            }
            other => panic!("not an install state: {other:?}"),
        })
        .collect()
}

fn missing() -> InstallState {
    InstallState::Missing {
        download_bytes: DOWNLOAD_BYTES,
    }
}

fn ready() -> InstallState {
    InstallState::Ready {
        version: VERSION.to_owned(),
    }
}

#[test]
fn the_variable_names_agents_and_drops_what_is_no_id() {
    let agents = agents_from(Some(" claude-acp , ,codex-acp,not an id"));
    assert_eq!(agents, BTreeSet::from([claude(), codex()]));
    assert!(agents_from(None).is_empty());
    assert!(agents_from(Some("")).is_empty());
}

#[test]
fn a_scripted_agent_starts_missing_then_installs_to_ready_with_no_s18_call() {
    let (fake, real, mut events) = fake(Duration::ZERO);
    assert_eq!(states(&mut events, 1), [missing()]);
    assert_eq!(fake.state(&claude()), Some(missing()));
    fake.install(&claude());
    assert_eq!(states(&mut events, 4), played());
    assert_eq!(fake.state(&claude()), Some(ready()));
    assert_eq!(
        fake.installed(&claude()),
        None,
        "no entry is written for it"
    );
    // a ready agent's install says Ready again, as S18's does
    fake.install(&claude());
    assert_eq!(states(&mut events, 1), [ready()]);
    fake.retry(&claude(), false);
    assert!(
        real.calls().is_empty(),
        "S18 is not asked: {:?}",
        real.calls()
    );
}

fn played() -> [InstallState; 4] {
    let start = InstallState::Downloading {
        received: 0,
        total: DOWNLOAD_BYTES,
    };
    let half = InstallState::Downloading {
        received: DOWNLOAD_BYTES / 2,
        total: DOWNLOAD_BYTES,
    };
    [start, half, InstallState::Installing, ready()]
}

#[test]
fn a_second_press_while_it_plays_joins_it() {
    let (fake, _real, mut events) = fake(Duration::from_millis(100));
    states(&mut events, 1);
    fake.install(&claude());
    fake.install(&claude());
    // a second install would say its own Downloading at 0 among these
    assert_eq!(states(&mut events, 4), played());
    assert!(events.try_recv().is_err(), "nothing more is said");
}

#[test]
fn other_agents_and_sign_ins_go_to_s18() {
    let (fake, real, _events) = fake(Duration::ZERO);
    fake.install(&codex());
    assert_eq!(fake.state(&codex()), None);
    fake.installed(&codex());
    fake.retry(&codex(), true);
    fake.sign_in(&claude(), Method::DeviceCode).unwrap();
    assert_eq!(
        real.calls(),
        ["install", "state", "installed", "retry", "sign_in"]
    );
}

#[test]
fn s18_s_install_states_for_a_scripted_agent_are_dropped() {
    let s18 = futures::stream::iter([
        AgentsEvent::Install {
            agent: claude(),
            state: InstallState::Missing { download_bytes: 1 },
        },
        AgentsEvent::Install {
            agent: codex(),
            state: InstallState::Missing { download_bytes: 2 },
        },
    ]);
    let real: Arc<dyn SetupAgents> = Arc::new(Real::default());
    let agents = BTreeSet::from([claude()]);
    let (_fake, source) = super::wrap_with(real, s18, &agents, &BTreeSet::new(), Duration::ZERO);
    let all: Vec<_> = futures::executor::block_on(source.collect());
    let said: Vec<_> = all
        .into_iter()
        .map(|event| match event {
            AgentsEvent::Install { agent, state } => (agent, state),
            other => panic!("not an install state: {other:?}"),
        })
        .collect();
    assert!(said.contains(&(codex(), InstallState::Missing { download_bytes: 2 })));
    assert!(!said.contains(&(claude(), InstallState::Missing { download_bytes: 1 })));
}

/// `E2E_FAKE_INSTALLED`: the agent starts `Ready` with no install run, S18 not asked, and a
/// press says `Ready` again.
#[test]
fn an_installed_agent_starts_ready_with_no_install_run() {
    let real = Arc::new(Real::default());
    let (fake, mut events) = FakeInstall::new(
        Arc::clone(&real) as Arc<dyn SetupAgents>,
        &BTreeSet::new(),
        &BTreeSet::from([claude()]),
        Duration::from_secs(3600),
    );
    assert_eq!(states(&mut events, 1), [ready()]);
    assert_eq!(fake.state(&claude()), Some(ready()));
    assert_eq!(
        fake.installed(&claude()),
        None,
        "no entry is written for it"
    );
    fake.install(&claude());
    assert_eq!(states(&mut events, 1), [ready()]);
    assert!(
        real.calls().is_empty(),
        "S18 is not asked: {:?}",
        real.calls()
    );
}
