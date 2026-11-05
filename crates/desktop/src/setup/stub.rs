//! The stand-in backends until R20 P5b puts S18's `Agents` behind [`SetupBackend`]:
//!
//! - [`NotWired`], the app's: it answers every command `internal` and publishes nothing
//!   (`Setup::not_wired` publishes its one `setup_info`).
//! - [`StubBackend`], the tests': it answers each action from a script a test sets, and
//!   publishes the script's events in order; with no script, it answers as `NotWired`.
//!
//! `StubBackend` keeps no key, only which agents have one, in one lock, so a
//! [`KeyWrite::OnlyIfAbsent`] write checks and stores in one step, as `agents`' key store must
//! in P5b. A `Secret` is dropped with its command when [`StubBackend::run`] returns; the calls
//! it records are the action and the agent alone.

#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
use clauseo_protocol::{AgentId, SetupEvent};
use clauseo_protocol::{CommandOutcome, ErrorCode, HostError, SetupAction, SetupCommand};

#[cfg(test)]
use super::admit::key_stored;
use super::{KeyWrite, SetupBackend, SetupEvents};
#[cfg(test)]
use crate::bridge::lock;
use crate::dialogs::BoxFuture;

/// The app's backend until R20 P5b: nothing is wired yet.
pub(crate) struct NotWired;

/// The answer to an action nothing runs yet.
fn not_wired(action: SetupAction) -> HostError {
    HostError::new(
        ErrorCode::Internal,
        format!("{action:?} is not wired to the agents yet (R20 P5b)"),
    )
}

impl SetupBackend for NotWired {
    fn run<'a>(
        &'a self,
        command: SetupCommand,
        _key_write: KeyWrite,
        _events: &'a SetupEvents,
    ) -> BoxFuture<'a, Result<CommandOutcome, HostError>> {
        Box::pin(std::future::ready(Err(not_wired(command.action()))))
    }
}

/// A scripted backend, for tests.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct StubBackend {
    state: Mutex<StubState>,
}

/// The scripts, the calls and the agents with a key.
#[cfg(test)]
#[derive(Default)]
struct StubState {
    scripts: BTreeMap<SetupAction, Script>,
    calls: Vec<(SetupAction, AgentId)>,
    keys: BTreeSet<AgentId>,
}

/// An action's answer and the events published before it.
#[cfg(test)]
#[derive(Clone)]
struct Script {
    answer: Result<CommandOutcome, HostError>,
    events: Vec<SetupEvent>,
}

#[cfg(test)]
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
        if action == SetupAction::SetAgentKey && key_write == KeyWrite::OnlyIfAbsent && stored {
            return (Err(key_stored(agent)), Vec::new());
        }
        let Some(script) = state.scripts.get(&action).cloned() else {
            return (Err(not_wired(action)), Vec::new());
        };
        if script.answer.is_ok() {
            match action {
                SetupAction::SetAgentKey => {
                    state.keys.insert(agent.clone());
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

#[cfg(test)]
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
