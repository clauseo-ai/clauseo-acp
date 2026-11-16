//! What the handle, the worker and the jobs share: the states, the installs and the emits.

use std::io;
use std::sync::{Arc, MutexGuard, PoisonError};
use std::time::Duration;

use clauseo_protocol::AgentId;

use super::space::Reserve;
use super::{Inner, Shared, Slot, Ticket};
use crate::state::{AgentsEvent, Failure, FailureKind, InstallState, Installed, PythonState};

impl Shared {
    pub(super) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `agent`'s finished install, when it is `Ready`.
    pub(crate) fn installed(&self, agent: &AgentId) -> Option<Installed> {
        self.lock().installed.get(agent).cloned()
    }

    /// Starts a job's thread named `name`, counted until its work ends (a panic too), unless
    /// the worker is closed: then nothing starts. The close and this check share the lock, so
    /// every thread started before `shutdown` is counted before it waits.
    pub(super) fn spawn_job(
        self: &Arc<Self>,
        name: String,
        work: impl FnOnce() + Send + 'static,
    ) -> io::Result<()> {
        let inner = self.lock();
        if inner.closed {
            return Ok(());
        }
        *self.jobs_lock() += 1;
        let running = Running(Arc::clone(self));
        let spawned = std::thread::Builder::new().name(name).spawn(move || {
            let _running = running;
            work();
        });
        // A thread that did not start dropped its closure, and `Running` with it.
        drop(inner);
        spawned.map(drop)
    }

    /// Waits, at most `wait`, until no job thread runs; whether none does.
    pub(super) fn wait_for_jobs(&self, wait: Duration) -> bool {
        let jobs = self.jobs_lock();
        let (jobs, _timeout) = self
            .job_ended
            .wait_timeout_while(jobs, wait, |jobs| *jobs > 0)
            .unwrap_or_else(PoisonError::into_inner);
        *jobs == 0
    }

    fn jobs_lock(&self) -> MutexGuard<'_, usize> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The Python runtime's state.
    pub(crate) fn python_state(&self) -> PythonState {
        self.lock().python.clone()
    }

    /// Records and sends the Python runtime's `state`: the worker's own events.
    pub(super) fn set_python(&self, state: PythonState) {
        Inner::set_python(&mut self.lock(), &self.events, state);
    }

    /// Records the size of Python's archive, for the `Missing` a paused Python download falls
    /// back to.
    pub(super) fn set_python_bytes(&self, bytes: u64) {
        self.lock().python_bytes = bytes;
    }

    /// Records and sends Python's `state` for the job of `ticket`, unless it was paused or
    /// superseded: then nothing happens and `false` comes back.
    pub(crate) fn emit_python_job(&self, ticket: &Ticket, state: PythonState) -> bool {
        let mut inner = self.lock();
        if ticket.cancel.is_cancelled() {
            return false;
        }
        if let PythonState::Downloading { received, .. } = state
            && let Some(slot) = inner.slot_mut(ticket)
        {
            slot.reserve.python_received = received;
        }
        Inner::set_python(&mut inner, &self.events, state);
        true
    }

    /// Makes the job of `ticket` the one installing Python, unless another job is (or it was
    /// paused or superseded): `false` then, and it ends with its agent's `Ready`.
    pub(crate) fn claim_python(&self, ticket: &Ticket) -> bool {
        let mut inner = self.lock();
        if ticket.cancel.is_cancelled() || inner.python_job.is_some() {
            return false;
        }
        inner.python_job = Some(ticket.job);
        true
    }

    /// Ends the job of `ticket` (its agent `Ready` already sent) with Python's last `state`,
    /// unless it was paused or superseded.
    pub(crate) fn end_python_job(&self, ticket: &Ticket, state: PythonState) {
        let mut inner = self.lock();
        if ticket.cancel.is_cancelled() {
            return;
        }
        inner.end_slot(ticket);
        Inner::set_python(&mut inner, &self.events, state);
    }

    /// The job of `ticket` panicked: unless paused or superseded, its slot and Python claim go,
    /// and its agent's install is `Failed` with a log saying so (an agent `Ready` already
    /// stays so, and Python, if this job was fetching it, goes back to `Missing`).
    pub(super) fn end_after_panic(&self, ticket: &Ticket) {
        let mut inner = self.lock();
        if ticket.cancel.is_cancelled() || inner.slot_mut(ticket).is_none() {
            return;
        }
        let python = inner.python_job == Some(ticket.job);
        inner.end_slot(ticket);
        tracing::warn!(agent = %ticket.agent, "agent install: the job's thread panicked");
        if matches!(
            inner.states.get(&ticket.agent),
            Some(InstallState::Ready { .. })
        ) {
            if python {
                let download_bytes = inner.python_bytes;
                Inner::set_python(
                    &mut inner,
                    &self.events,
                    PythonState::Missing { download_bytes },
                );
            }
            return;
        }
        let mut log = crate::log::InstallLog::new(&ticket.agent);
        log.push("the install stopped unexpectedly".to_owned());
        log.push(format!(
            "result: {} after 1 tries",
            FailureKind::Failed.word()
        ));
        let failure = Failure {
            kind: FailureKind::Failed,
            need_mb: None,
            free_mb: None,
            log,
        };
        Inner::set(
            &mut inner,
            &self.events,
            &ticket.agent,
            InstallState::Failed(failure),
            None,
        );
    }

    /// Records `tries` as `agent`'s last install job's, just before that job's last event.
    pub(crate) fn set_tries(&self, agent: &AgentId, tries: u32) {
        self.lock().tries.insert(agent.clone(), tries);
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
        if ends {
            inner.end_slot(ticket);
        } else if let Some(slot) = inner.slot_mut(ticket)
            && matches!(state, InstallState::Ready { .. })
        {
            // The agent's files are on the disk: only Python's part, if any, is to come.
            slot.reserve.agent = 0;
        }
        let ready = matches!(state, InstallState::Ready { .. });
        if ready || state == InstallState::Installing {
            let word = if ready { "ready" } else { "installing" };
            // The lifecycle target: host.log keeps INFO only from `clauseo_*::lifecycle`.
            tracing::info!(
                target: "clauseo_agents::lifecycle",
                agent = %ticket.agent,
                state = word,
                "agent install: state sent"
            );
        }
        Inner::set(&mut inner, &self.events, &ticket.agent, state, installed);
        drop(inner);
        // After the lock: a queued sign-in reads the install again once kicked.
        if ready {
            self.signin.install_ready(&ticket.agent);
        }
        true
    }
}

/// A running job thread, counted in `Shared::jobs` until it is dropped.
struct Running(Arc<Shared>);

impl Drop for Running {
    fn drop(&mut self) {
        let mut jobs = self.0.jobs_lock();
        *jobs = jobs.saturating_sub(1);
        drop(jobs);
        self.0.job_ended.notify_all();
    }
}

impl Default for Inner {
    fn default() -> Inner {
        Inner {
            states: std::collections::BTreeMap::new(),
            python: PythonState::Unavailable,
            python_bytes: 0,
            python_began: None,
            ticks: super::Ticks::default(),
            installed: std::collections::BTreeMap::new(),
            jobs: std::collections::BTreeMap::new(),
            python_job: None,
            next_job: 0,
            tries: std::collections::BTreeMap::new(),
            closed: false,
        }
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

    /// Records and sends Python's `state`.
    pub(super) fn set_python(
        inner: &mut Inner,
        events: &async_channel::Sender<AgentsEvent>,
        state: PythonState,
    ) {
        let began = &mut inner.python_began;
        super::python_log::changed(&inner.python, &state, began, &inner.ticks);
        inner.python = state.clone();
        drop(events.try_send(AgentsEvent::Python { state }));
    }

    /// `ticket`'s slot, while it is its agent's running job.
    pub(super) fn slot_mut(&mut self, ticket: &Ticket) -> Option<&mut Slot> {
        self.jobs
            .get_mut(&ticket.agent)
            .filter(|slot| slot.ticket.job == ticket.job)
    }

    /// Ends `ticket`'s slot, and its claim on Python, when it is still its agent's running job.
    pub(super) fn end_slot(&mut self, ticket: &Ticket) {
        if self.slot_mut(ticket).is_some() {
            self.jobs.remove(&ticket.agent);
        }
        if self.python_job == Some(ticket.job) {
            self.python_job = None;
        }
    }

    /// Adds `ticket` as its agent's running job, with nothing reserved yet.
    pub(super) fn add_job(&mut self, ticket: Ticket) {
        let agent = ticket.agent.clone();
        let slot = Slot {
            ticket,
            reserve: Reserve::default(),
        };
        self.jobs.insert(agent, slot);
    }

    /// Whether `agent` has a running job.
    pub(super) fn running(&self, agent: &AgentId) -> bool {
        self.jobs.contains_key(agent)
    }

    /// The agent whose job installs Python now, if any.
    pub(super) fn python_owner(&self) -> Option<AgentId> {
        let job = self.python_job?;
        self.jobs
            .values()
            .find(|slot| slot.ticket.job == job)
            .map(|slot| slot.ticket.agent.clone())
    }

    /// Stops `agent`'s running job, if any; no other agent's. It becomes `Paused`, unless
    /// `superseded` (a new job of the same agent, a retry while it waits: quiet). A job past its
    /// agent's `Ready`, installing Python, leaves the agent `Ready`; Python's download goes
    /// back to `Missing`, its partial file kept.
    pub(super) fn stop_job(
        &mut self,
        events: &async_channel::Sender<AgentsEvent>,
        agent: &AgentId,
        superseded: bool,
    ) {
        let Some(running) = self.jobs.remove(agent) else {
            return;
        };
        running.ticket.cancel.cancel();
        if self.python_job == Some(running.ticket.job) {
            self.python_job = None;
            let going = matches!(
                self.python,
                PythonState::Waiting { .. }
                    | PythonState::Downloading { .. }
                    | PythonState::Installing
            );
            if going {
                let download_bytes = self.python_bytes;
                Inner::set_python(self, events, PythonState::Missing { download_bytes });
            }
        }
        let ready = matches!(self.states.get(agent), Some(InstallState::Ready { .. }));
        if !ready && !superseded {
            Inner::set(self, events, agent, InstallState::Paused, None);
        }
    }
}
