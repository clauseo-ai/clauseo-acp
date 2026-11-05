//! [`StubBackend`], the tests' setup backend: it answers each action from a script a test
//! sets, and publishes the script's events in order; with no script, it answers `internal`.
//!
//! It keeps no key, only which agents have one, in one lock, so a [`KeyWrite::OnlyIfAbsent`]
//! write checks and stores in one step, as the app's backend does through
//! `Keychain::store_if_absent`. A `Secret` is dropped with its command when
//! [`StubBackend::run`] returns; the calls it records are the action and the agent alone.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use clauseo_protocol::{
    AgentId, CommandOutcome, ErrorCode, HostError, SetupAction, SetupCommand, SetupEvent,
};

use super::admit::key_stored;
use super::{KeyWrite, SetupBackend, SetupEvents};
use crate::bridge::lock;
use crate::dialogs::BoxFuture;

/// The answer to an action no script covers.
fn unscripted(action: SetupAction) -> HostError {
    HostError::new(ErrorCode::Internal, format!("{action:?} has no script"))
}

/// A scripted backend, for tests.
#[derive(Default)]
pub(crate) struct StubBackend {
    state: Mutex<StubState>,
}

/// The scripts, the calls and the agents with a key.
#[derive(Default)]
struct StubState {
    scripts: BTreeMap<SetupAction, Script>,
    calls: Vec<(SetupAction, AgentId)>,
    keys: BTreeSet<AgentId>,
    /// Each key write that stored, in order: the agent and how it was written.
    stores: Vec<(AgentId, KeyWrite)>,
    /// Whether reading the keychain fails, as a locked keychain or a denied prompt does.
    reads_fail: bool,
}

/// An action's answer and the events published before it.
#[derive(Clone)]
struct Script {
    answer: Result<CommandOutcome, HostError>,
    events: Vec<SetupEvent>,
}

impl StubBackend {
    /// Answers every later `action` with `answer`, after publishing `events` in order.
    pub(crate) fn script(
        &self,
        action: SetupAction,
        answer: Result<CommandOutcome, HostError>,
        events: Vec<SetupEvent>,
    ) {
        lock(&self.state)
            .scripts
            .insert(action, Script { answer, events });
    }

    /// The calls that reached it, in order: each action and its agent.
    pub(crate) fn calls(&self) -> Vec<(SetupAction, AgentId)> {
        lock(&self.state).calls.clone()
    }

    /// From now on, reading the keychain fails (or no longer does): an `OnlyIfAbsent` key is
    /// refused with `internal` and nothing is stored, since a failed read is not "absent".
    pub(crate) fn fail_reads(&self, fail: bool) {
        lock(&self.state).reads_fail = fail;
    }

    /// Each key write that stored, in order.
    pub(crate) fn stores(&self) -> Vec<(AgentId, KeyWrite)> {
        lock(&self.state).stores.clone()
    }

    /// Whether a `SetAgentKey` for `agent` stored a key, and no `ClearAgentKey` removed it.
    pub(crate) fn has_key(&self, agent: &AgentId) -> bool {
        lock(&self.state).keys.contains(agent)
    }

    /// The answer to `command` and the events to publish, in one step under the lock.
    fn answer(
        &self,
        command: &SetupCommand,
        key_write: KeyWrite,
    ) -> (Result<CommandOutcome, HostError>, Vec<SetupEvent>) {
        let action = command.action();
        let agent = command.agent();
        let mut state = lock(&self.state);
        state.calls.push((action, agent.clone()));
        let stored = state.keys.contains(agent);
        if action == SetupAction::SetAgentKey && key_write == KeyWrite::OnlyIfAbsent {
            if state.reads_fail {
                let refused = HostError::new(
                    ErrorCode::Internal,
                    format!("the keychain refused access to {agent}'s API key"),
                );
                return (Err(refused), Vec::new());
            }
            if stored {
                return (Err(key_stored(agent)), Vec::new());
            }
        }
        let Some(script) = state.scripts.get(&action).cloned() else {
            return (Err(unscripted(action)), Vec::new());
        };
        if script.answer.is_ok() {
            match action {
                SetupAction::SetAgentKey => {
                    state.keys.insert(agent.clone());
                    state.stores.push((agent.clone(), key_write));
                }
                SetupAction::ClearAgentKey => {
                    state.keys.remove(agent);
                }
                _ => {}
            }
        }
        (script.answer, script.events)
    }
}

impl SetupBackend for StubBackend {
    fn run<'a>(
        &'a self,
        command: SetupCommand,
        key_write: KeyWrite,
        events: &'a SetupEvents,
    ) -> BoxFuture<'a, Result<CommandOutcome, HostError>> {
        let (answer, published) = self.answer(&command, key_write);
        // The command, and any secret in it, goes here: nothing keeps it.
        drop(command);
        for event in published {
            events.publish(event);
        }
        Box::pin(std::future::ready(answer))
    }
}
