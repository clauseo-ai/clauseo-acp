//! One agent's sign-in events as the tests read them ([`Events`]), and a sign-in event
//! ([`Seen`]).

use std::sync::Mutex;

use clauseo_agents::{AgentsEvent, InstallState, SigninNote, SigninState};
use clauseo_protocol::AgentId;

use crate::support::lock;

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
    /// A reader of `agent`'s events from `rx`.
    pub(crate) fn new(agent: AgentId, rx: async_channel::Receiver<AgentsEvent>) -> Events {
        Events {
            rx,
            agent,
            notes: Mutex::default(),
        }
    }

    /// Reads up to the agent's next install state, which must be `Ready`.
    pub(crate) fn until_install_ready(&self) {
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

    /// The agent's sign-in notes read so far, without the log's step notes ([`Events::log_notes`]).
    pub(crate) fn notes(&self) -> Vec<SigninNote> {
        lock(&self.notes)
            .iter()
            .copied()
            .filter(|note| {
                !matches!(
                    note,
                    SigninNote::CliResolved { .. }
                        | SigninNote::LoginExited { .. }
                        | SigninNote::StatusConfirmed
                        | SigninNote::StatusNotConfirmed { .. }
                )
            })
            .collect()
    }

    /// Every sign-in note read so far, the log's step notes included.
    pub(crate) fn log_notes(&self) -> Vec<SigninNote> {
        lock(&self.notes).clone()
    }

    /// Every event already sent, without waiting, Install events included.
    pub(crate) fn drain(&self) -> Vec<AgentsEvent> {
        std::iter::from_fn(|| self.rx.try_recv().ok()).collect()
    }
}
