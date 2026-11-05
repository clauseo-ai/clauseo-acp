//! What the handle, the worker and the jobs share: the states, the installs and the emits.

use std::sync::{MutexGuard, PoisonError};

use clauseo_protocol::AgentId;

use super::{Inner, Shared, Ticket};
use crate::state::{AgentsEvent, InstallState, Installed};

impl Shared {
    pub(super) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `agent`'s finished install, when it is `Ready`.
    pub(crate) fn installed(&self, agent: &AgentId) -> Option<Installed> {
        self.lock().installed.get(agent).cloned()
    }

    /// Records and sends `state` for `agent`: the worker's own events.
    pub(super) fn emit(&self, agent: &AgentId, state: InstallState, installed: Option<Installed>) {
        let ready = matches!(state, InstallState::Ready { .. });
        Inner::set(&mut self.lock(), &self.events, agent, state, installed);
        if ready {
            self.signin.install_ready(agent);
        }
    }

    /// Records and sends `state` for the job of `ticket`, unless it was paused or superseded:
    /// then nothing happens and `false` comes back. `ends` marks the job's last event (`Ready`
    /// or `Failed`), after which it is no longer the running job.
    pub(crate) fn emit_job(
        &self,
        ticket: &Ticket,
        state: InstallState,
        installed: Option<Installed>,
        ends: bool,
    ) -> bool {
        let mut inner = self.lock();
        if ticket.cancel.is_cancelled() {
            return false;
        }
        if ends
            && inner
                .current
                .as_ref()
                .is_some_and(|current| current.job == ticket.job)
        {
            inner.current = None;
        }
        let ready = matches!(state, InstallState::Ready { .. });
        Inner::set(&mut inner, &self.events, &ticket.agent, state, installed);
        drop(inner);
        // After the lock: a queued sign-in reads the install again once kicked.
        if ready {
            self.signin.install_ready(&ticket.agent);
        }
        true
    }
}

impl Inner {
    pub(super) fn set(
        inner: &mut Inner,
        events: &async_channel::Sender<AgentsEvent>,
        agent: &AgentId,
        state: InstallState,
        installed: Option<Installed>,
    ) {
        match installed {
            Some(installed) => {
                inner.installed.insert(agent.clone(), installed);
            }
            None if !matches!(state, InstallState::Ready { .. }) => {
                inner.installed.remove(agent);
            }
            None => {}
        }
        inner.states.insert(agent.clone(), state.clone());
        // Fails only when every receiver is gone; nobody is listening then.
        drop(events.try_send(AgentsEvent::Install {
            agent: agent.clone(),
            state,
        }));
    }

    /// Stops the running job. It becomes `Paused`, unless it is `next`'s own job (a retry
    /// while it waits), which is superseded quietly.
    pub(super) fn stop_current(
        &mut self,
        events: &async_channel::Sender<AgentsEvent>,
        next: Option<&AgentId>,
    ) {
        let Some(running) = self.current.take() else {
            return;
        };
        running.cancel.cancel();
        if next != Some(&running.agent) {
            Inner::set(self, events, &running.agent, InstallState::Paused, None);
        }
    }
}
