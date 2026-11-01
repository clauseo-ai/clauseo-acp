//! [`Limits`]: the refresh policy and the deadlines, over the model and the view.
//!
//! Every input names its agent; an agent not passed to [`Limits::new`] has no state, so its
//! inputs do nothing.

mod deadlines;
mod triggers;

use std::collections::BTreeMap;

use clauseo_protocol::{AccountLimits, AgentKind, LimitsView, Timestamp};

use crate::model::{AgentState, LimitHit, LiveSignal, apply_live, apply_read};
use crate::readers::ReadOutcome;
use crate::view::agent_view;

pub(crate) use triggers::Reads;
use triggers::request_read;

/// The limits model: pure, no I/O and no clock. The host feeds it [`Input`]s with `now` and
/// carries out the [`Action`]s it returns.
#[derive(Debug)]
pub struct Limits {
    /// One state per agent passed to `new`, in the order given.
    agents: Vec<AgentState>,
    /// The host has limits subscribers; deadlines run only then.
    watching: bool,
}

/// What happened, as the host tells it.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// The host has limits subscribers (`any: true`), or has none left.
    Watching {
        /// Whether anyone watches.
        any: bool,
    },
    /// A window got focus, or a meter or the settings page opened: one agent's, or every one.
    Shown {
        /// The agent, or `None` for every agent.
        agent: Option<AgentKind>,
    },
    /// The lawyer asked to try again: one agent, or every one.
    TryAgain {
        /// The agent, or `None` for every agent.
        agent: Option<AgentKind>,
    },
    /// A turn of `agent` ended.
    TurnEnded {
        /// The agent.
        agent: AgentKind,
        /// Read at once: an at-limit stop, or R16's Codex fallback.
        read_now: bool,
    },
    /// A permission or elicitation of `agent` is pending (`asking: true`), or no longer.
    Asking {
        /// The agent.
        agent: AgentKind,
        /// Whether a question is pending.
        asking: bool,
    },
    /// A live rate-limit signal from `agent`.
    Live {
        /// The agent.
        agent: AgentKind,
        /// The signal.
        signal: LiveSignal,
    },
    /// A read the model asked for has ended.
    ReadDone {
        /// The agent.
        agent: AgentKind,
        /// How it ended.
        outcome: ReadOutcome,
    },
    /// The time [`Limits::next_deadline`] gave has come.
    Deadline,
}

/// What the host does next.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Start a read of this agent's limits, and send its outcome back as [`Input::ReadDone`].
    Read(AgentKind),
    /// Persist these limits, publish them, and publish the view again.
    Changed(AccountLimits),
    /// Only the view changed (a stale mark, `ResetPassed`, `Watching`): publish it again.
    ViewChanged,
}

impl Limits {
    /// The model at start, from the cached limits and the agents that have a settings entry,
    /// with the reads to start.
    pub fn new(
        cached: Vec<AccountLimits>,
        agents: &[AgentKind],
        now: Timestamp,
    ) -> (Limits, Vec<Action>) {
        // The last cached entry of each agent wins.
        let mut cached: BTreeMap<AgentKind, AccountLimits> = cached
            .into_iter()
            .map(|limits| (limits.agent, limits))
            .collect();
        let mut states: Vec<AgentState> = Vec::with_capacity(agents.len());
        let mut actions = Vec::with_capacity(agents.len());
        for &agent in agents {
            if states.iter().any(|state| state.agent == agent) {
                continue;
            }
            let mut state = match cached.remove(&agent) {
                Some(limits) => AgentState::from_cache(agent, limits, now),
                None => AgentState::loading(agent),
            };
            request_read(&mut state, now, &mut actions);
            states.push(state);
        }
        (
            Limits {
                agents: states,
                watching: false,
            },
            actions,
        )
    }

    /// Takes one input at `now`.
    pub fn input(&mut self, input: Input, now: Timestamp) -> Vec<Action> {
        let mut actions = Vec::new();
        match input {
            Input::Watching { any } => {
                self.watching = any;
                if any {
                    for state in &mut self.agents {
                        deadlines::sweep(state, now, &mut actions);
                    }
                    actions.push(Action::ViewChanged);
                }
            }
            Input::Shown { agent } => {
                for state in self.targets(agent) {
                    triggers::shown(state, now, &mut actions);
                }
            }
            Input::TryAgain { agent } => {
                for state in self.targets(agent) {
                    request_read(state, now, &mut actions);
                }
            }
            Input::TurnEnded { agent, read_now } => {
                if let Some(state) = self.state_mut(agent) {
                    triggers::turn_ended(state, read_now, now, &mut actions);
                }
            }
            Input::Asking { agent, asking } => {
                if let Some(state) = self.state_mut(agent) {
                    triggers::asking(state, asking, now, &mut actions);
                }
            }
            Input::Live { agent, signal } => {
                if let Some(state) = self.state_mut(agent) {
                    actions.extend(apply_live(state, &signal, now));
                }
            }
            Input::ReadDone { agent, outcome } => {
                if let Some(state) = self.state_mut(agent) {
                    state.reads.in_flight = false;
                    actions.extend(apply_read(state, outcome, now));
                }
            }
            Input::Deadline => {
                let mut changed = false;
                for state in &mut self.agents {
                    changed |= deadlines::sweep(state, now, &mut actions);
                }
                if changed {
                    actions.push(Action::ViewChanged);
                }
            }
        }
        actions
    }

    /// The view at `now`.
    pub fn view(&self, now: Timestamp) -> LimitsView {
        LimitsView(
            self.agents
                .iter()
                .map(|state| (state.agent, agent_view(state, now)))
                .collect(),
        )
    }

    /// When the host should send [`Input::Deadline`]; `None` while nobody watches.
    pub fn next_deadline(&self) -> Option<Timestamp> {
        if self.watching {
            deadlines::next_deadline(&self.agents)
        } else {
            None
        }
    }

    /// The next shown reset time of `agent`, when its numbers will change.
    pub fn next_read_at(&self, agent: AgentKind) -> Option<Timestamp> {
        self.state(agent).and_then(deadlines::next_read_at)
    }

    /// The limit `agent` is at, if any.
    pub fn at_limit(&self, agent: AgentKind) -> Option<LimitHit> {
        self.state(agent).and_then(crate::model::at_limit)
    }

    /// `agent`'s state, when it was passed to `new`.
    fn state(&self, agent: AgentKind) -> Option<&AgentState> {
        self.agents.iter().find(|state| state.agent == agent)
    }

    /// `agent`'s state, to change it.
    fn state_mut(&mut self, agent: AgentKind) -> Option<&mut AgentState> {
        self.agents.iter_mut().find(|state| state.agent == agent)
    }

    /// The states an input for `agent` (`None` for every agent) reaches.
    fn targets(&mut self, agent: Option<AgentKind>) -> impl Iterator<Item = &mut AgentState> {
        self.agents
            .iter_mut()
            .filter(move |state| agent.is_none_or(|agent| agent == state.agent))
    }
}
