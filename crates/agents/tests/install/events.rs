//! The events' reader the install tests share: agents' states and Python's, read apart.

use std::cell::RefCell;
use std::collections::VecDeque;

use clauseo_agents::testing::trace;
use clauseo_agents::{AgentsEvent, InstallState, PythonState};
use clauseo_protocol::AgentId;

use crate::support::{agent_a, agent_b};

/// The events' receiver, read by blocking. Python's events and the agents' are read apart:
/// each reader keeps the other kind's events it passes for the other.
pub(crate) struct Events {
    pub(crate) receiver: async_channel::Receiver<AgentsEvent>,
    installs: RefCell<VecDeque<(AgentId, InstallState)>>,
    pythons: RefCell<VecDeque<PythonState>>,
}

impl Events {
    pub(crate) fn new(receiver: async_channel::Receiver<AgentsEvent>) -> Events {
        Events {
            receiver,
            installs: RefCell::new(VecDeque::new()),
            pythons: RefCell::new(VecDeque::new()),
        }
    }

    /// The next event off the channel, an install's or Python's.
    fn recv(&self) -> AgentsEvent {
        futures::executor::block_on(self.receiver.recv()).expect("the events closed")
    }

    /// The next agent's event.
    pub(crate) fn next(&self) -> (AgentId, InstallState) {
        if let Some(kept) = self.installs.borrow_mut().pop_front() {
            return kept;
        }
        loop {
            match self.recv() {
                AgentsEvent::Install { agent, state } => {
                    trace_state(&agent, &state);
                    return (agent, state);
                }
                AgentsEvent::Python { state } => self.pythons.borrow_mut().push_back(state),
                event => panic!("an event of another kind: {event:?}"),
            }
        }
    }

    #[cfg(unix)]
    /// The next Python event.
    pub(crate) fn next_python(&self) -> PythonState {
        if let Some(kept) = self.pythons.borrow_mut().pop_front() {
            return kept;
        }
        loop {
            match self.recv() {
                AgentsEvent::Python { state } => {
                    trace(format_args!("event: python {state:?}"));
                    return state;
                }
                AgentsEvent::Install { agent, state } => {
                    trace_state(&agent, &state);
                    self.installs.borrow_mut().push_back((agent, state));
                }
                event => panic!("an event of another kind: {event:?}"),
            }
        }
    }

    #[cfg(unix)]
    /// Python's events up to and including the first for which `done` holds.
    pub(crate) fn until_python(&self, done: impl Fn(&PythonState) -> bool) -> Vec<PythonState> {
        let mut seen = Vec::new();
        loop {
            let state = self.next_python();
            let last = done(&state);
            seen.push(state);
            if last {
                return seen;
            }
        }
    }

    #[cfg(unix)]
    /// Python's events up to its `Ready` or `Failed`, and that last state.
    pub(crate) fn until_python_end(&self) -> (Vec<PythonState>, PythonState) {
        let seen = self.until_python(|state| {
            matches!(state, PythonState::Ready { .. } | PythonState::Failed(_))
        });
        let last = seen.last().unwrap().clone();
        (seen, last)
    }

    /// Events up to and including the first of `agent`'s for which `done` holds.
    pub(crate) fn until(
        &self,
        agent: &AgentId,
        done: impl Fn(&InstallState) -> bool,
    ) -> Vec<(AgentId, InstallState)> {
        let mut seen = Vec::new();
        loop {
            let (id, state) = self.next();
            let last = id == *agent && done(&state);
            seen.push((id, state));
            if last {
                return seen;
            }
        }
    }

    /// Events up to `agent`'s `Ready` or `Failed`, and that last state.
    pub(crate) fn until_end(
        &self,
        agent: &AgentId,
    ) -> (Vec<(AgentId, InstallState)>, InstallState) {
        let seen = self.until(agent, |state| {
            matches!(state, InstallState::Ready { .. } | InstallState::Failed(_))
        });
        let last = seen.last().unwrap().1.clone();
        (seen, last)
    }

    /// Both agents' first states, A's then B's.
    pub(crate) fn first_states(&self) -> (InstallState, InstallState) {
        let (a, first) = self.next();
        let (b, second) = self.next();
        assert_eq!((a, b), (agent_a(), agent_b()));
        (first, second)
    }

    /// Every agent's event already sent, without waiting.
    pub(crate) fn drain(&self) -> Vec<(AgentId, InstallState)> {
        let mut out: Vec<_> = self.installs.borrow_mut().drain(..).collect();
        out.extend(
            std::iter::from_fn(|| self.receiver.try_recv().ok()).filter_map(|event| match event {
                AgentsEvent::Install { agent, state } => Some((agent, state)),
                _ => None,
            }),
        );
        out
    }
}

/// Traces `agent`'s new state by its kind (`Downloading`, `Ready`, …), not its fields.
fn trace_state(agent: &AgentId, state: &InstallState) {
    let text = format!("{state:?}");
    let kind = text
        .split(|ch: char| !ch.is_alphanumeric())
        .next()
        .unwrap_or_default();
    trace(format_args!("event: {agent} {kind}"));
}

/// `agent`'s states among `events`.
pub(crate) fn states_of(events: &[(AgentId, InstallState)], agent: &AgentId) -> Vec<InstallState> {
    events
        .iter()
        .filter(|(id, _)| id == agent)
        .map(|(_, state)| state.clone())
        .collect()
}
