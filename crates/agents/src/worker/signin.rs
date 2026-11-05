//! Sign-in's share of the worker: each agent's sign-in state and running job ([`SigninHub`]), the
//! checks and slot reservation behind the sign-in calls on `Agents`, and the worker's start of a
//! job ([`start_job`]). [`crate::signin::job`] has the model.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use clauseo_process::Timer;
use clauseo_protocol::AgentId;

use super::{Ticket, Worker};
use crate::cancel::Cancel;
use crate::config::AgentsConfig;
use crate::signin::env::Vendor;
use crate::signin::job::{self, SigninJob, Task};
use crate::signin::{
    Found, Method, PasteCode, SigninCode, SigninError, SigninNote, SigninState, Source, vendor_of,
};
use crate::state::{AgentsEvent, Installed};

/// Every agent's sign-in: what the handle, the worker and the sign-in jobs share.
#[derive(Debug)]
pub(crate) struct SigninHub {
    /// The snapshot's agents Clauseo signs in to, read once at start.
    agents: BTreeMap<AgentId, SigninAgent>,
    inner: Mutex<Inner>,
    events: async_channel::Sender<AgentsEvent>,
    /// Held while anything may write Codex's login file in an agent's home: a Codex `login` or
    /// `logout` run from its guard to its end (the process gone), and "Use it" from its last
    /// check through the link to the `login-source` record. So a "Use it" that a newer job
    /// replaced either finished (and undid) its link before that job's guard looks, or sees it
    /// was replaced and links nothing; and no link is made while a replaced login still runs.
    codex_files: futures::lock::Mutex<()>,
}

/// An agent the snapshot has and Clauseo signs in to.
#[derive(Debug, Clone)]
struct SigninAgent {
    vendor: Vendor,
    /// `SigninConfig::homes`' entry, if the caller gave one.
    home: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct Inner {
    /// Each agent's last state.
    states: BTreeMap<AgentId, SigninState>,
    /// Each agent's running job.
    jobs: BTreeMap<AgentId, Slot>,
    next_job: u64,
}

/// An agent's running job: its ticket, its task, where pasted codes go, and, for a task that
/// may queue, its kick on each install `Ready`.
#[derive(Debug)]
struct Slot {
    ticket: Ticket,
    task: Task,
    paste: Option<async_channel::Sender<PasteCode>>,
    kick: Option<async_channel::Sender<()>>,
}

/// A reserved job, for the worker to start.
#[derive(Debug)]
pub(crate) struct SigninStart {
    ticket: Ticket,
    task: Task,
    vendor: Vendor,
    /// The install when the call was made; a task that queues reads it again before it starts.
    installed: Option<Installed>,
    home: PathBuf,
    paste: Option<async_channel::Receiver<PasteCode>>,
    kick: Option<async_channel::Receiver<()>>,
}

impl SigninHub {
    /// The snapshot's agents with a sign-in, and their homes from `config`.
    pub(super) fn new(
        config: &AgentsConfig,
        events: async_channel::Sender<AgentsEvent>,
    ) -> SigninHub {
        let agents = config
            .snapshot
            .agents()
            .filter_map(|entry| {
                let vendor = vendor_of(&entry.id)?;
                let home = config.signin.homes.get(&entry.id).cloned();
                Some((entry.id.clone(), SigninAgent { vendor, home }))
            })
            .collect();
        SigninHub {
            agents,
            inner: Mutex::new(Inner::default()),
            events,
            codex_files: futures::lock::Mutex::new(()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `Agents::sign_in`'s checks, then the slot: `installed` is the agent's install, if ready.
    pub(super) fn sign_in(
        &self,
        agent: &AgentId,
        method: Method,
        installed: Option<Installed>,
    ) -> Result<Option<SigninStart>, SigninError> {
        let vendor = self.vendor(agent)?;
        let fits = matches!(
            (vendor, method),
            (Vendor::Claude, Method::ClaudeAi | Method::Console)
                | (Vendor::Codex, Method::Browser | Method::DeviceCode)
        );
        if !fits {
            return Err(SigninError::WrongMethod {
                agent: agent.clone(),
                method,
            });
        }
        self.reserve(agent, vendor, Task::SignIn(method), installed)
    }

    /// `Agents::sign_out`'s checks (never a reused login), then the slot.
    pub(super) fn sign_out(
        &self,
        agent: &AgentId,
        installed: Option<Installed>,
    ) -> Result<Option<SigninStart>, SigninError> {
        let vendor = self.vendor(agent)?;
        if self.source(agent) == Some(Source::Reused) {
            return Err(SigninError::Reused(agent.clone()));
        }
        self.reserve(agent, vendor, Task::SignOut, installed)
    }

    /// `Agents::try_again`'s checks (Codex alone has a callback port to wait for), then the
    /// slot.
    pub(super) fn try_again(
        &self,
        agent: &AgentId,
        installed: Option<Installed>,
    ) -> Result<Option<SigninStart>, SigninError> {
        let vendor = self.vendor(agent)?;
        if vendor != Vendor::Codex {
            return Err(SigninError::WrongMethod {
                agent: agent.clone(),
                method: Method::Browser,
            });
        }
        self.reserve(agent, vendor, Task::TryAgain, installed)
    }

    /// `Agents::check_status`'s checks, then the slot unless a job is running.
    pub(super) fn check_status(
        &self,
        agent: &AgentId,
        installed: Option<Installed>,
    ) -> Result<Option<SigninStart>, SigninError> {
        let vendor = self.vendor(agent)?;
        self.reserve(agent, vendor, Task::Status, installed)
    }

    /// `Agents::find_login`'s checks, then the slot.
    pub(super) fn find_login(
        &self,
        agent: &AgentId,
        installed: Option<Installed>,
    ) -> Result<Option<SigninStart>, SigninError> {
        let vendor = self.vendor(agent)?;
        self.reserve(agent, vendor, Task::FindLogin, installed)
    }

    /// `Agents::use_found_login`'s checks (a `LoginFound` was shown), then the slot.
    pub(super) fn use_found(
        &self,
        agent: &AgentId,
        installed: Option<Installed>,
    ) -> Result<Option<SigninStart>, SigninError> {
        let vendor = self.vendor(agent)?;
        if !matches!(self.state(agent), Some(SigninState::LoginFound { .. })) {
            return Err(SigninError::NothingFound(agent.clone()));
        }
        self.reserve(agent, vendor, Task::UseFound, installed)
    }

    /// `Agents::use_different_account`'s checks (never from a reused login, which is left with
    /// `stop_using_login` first), then the slot.
    pub(super) fn use_different(
        &self,
        agent: &AgentId,
        installed: Option<Installed>,
    ) -> Result<Option<SigninStart>, SigninError> {
        let vendor = self.vendor(agent)?;
        if self.source(agent) == Some(Source::Reused) {
            return Err(SigninError::Reused(agent.clone()));
        }
        self.reserve(agent, vendor, Task::UseDifferent, installed)
    }

    /// `Agents::stop_using_login`'s checks (never Clauseo's own login), then the slot.
    pub(super) fn stop_using(
        &self,
        agent: &AgentId,
        why: Option<Found>,
        installed: Option<Installed>,
    ) -> Result<Option<SigninStart>, SigninError> {
        let vendor = self.vendor(agent)?;
        if self.source(agent) == Some(Source::Own) {
            return Err(SigninError::NotReused(agent.clone()));
        }
        self.reserve(agent, vendor, Task::StopUsing(why), installed)
    }

    /// Cancels `agent`'s sign-in run; its job reports `Failed { code: Cancelled }`.
    pub(super) fn cancel(&self, agent: &AgentId) -> Result<(), SigninError> {
        let inner = self.lock();
        match inner.jobs.get(agent) {
            Some(slot) if slot.task.signs_in() => {
                slot.ticket.cancel.cancel();
                Ok(())
            }
            _ => Err(SigninError::NotRunning(agent.clone())),
        }
    }

    /// Whether `agent`'s job is one [`Self::cancel`] would end.
    pub(super) fn going(&self, agent: &AgentId) -> bool {
        let inner = self.lock();
        inner
            .jobs
            .get(agent)
            .is_some_and(|slot| slot.task.signs_in())
    }

    /// Hands `code` to `agent`'s sign-in run, which writes it to the login's stdin. Codex's
    /// logins read no code (the paste route is Claude's `claude.ai` and console logins), so a
    /// code for Codex is refused with `WrongMethod`, never taken and dropped.
    pub(super) fn paste(&self, agent: &AgentId, code: PasteCode) -> Result<(), SigninError> {
        if self.vendor(agent)? == Vendor::Codex {
            return Err(SigninError::WrongMethod {
                agent: agent.clone(),
                method: Method::ClaudeAi,
            });
        }
        let inner = self.lock();
        // No run reads stdin while the sign-in is queued.
        let queued = inner.states.get(agent) == Some(&SigninState::Queued);
        let sender = inner
            .jobs
            .get(agent)
            .filter(|slot| !queued && !slot.ticket.cancel.is_cancelled())
            .and_then(|slot| slot.paste.as_ref());
        match sender.map(|sender| sender.try_send(code)) {
            Some(Ok(())) => Ok(()),
            // A refused code is dropped, zeroed, inside the error.
            Some(Err(_)) | None => Err(SigninError::NotRunning(agent.clone())),
        }
    }

    /// `agent`'s last state.
    pub(crate) fn state(&self, agent: &AgentId) -> Option<SigninState> {
        self.lock().states.get(agent).cloned()
    }

    fn vendor(&self, agent: &AgentId) -> Result<Vendor, SigninError> {
        self.agents
            .get(agent)
            .map(|known| known.vendor)
            .ok_or_else(|| SigninError::UnknownAgent(agent.clone()))
    }

    /// Checks `agent` can run `task` and takes its job slot. A sign-in or sign-out stops the
    /// running job (silently: it is no longer the agent's); a status check with a job running
    /// reserves nothing, since that job reports. A task that starts a login run
    /// ([`Task::queues`]) takes the slot with no install ready, and queues; any other is refused
    /// with `NotReady`.
    fn reserve(
        &self,
        agent: &AgentId,
        vendor: Vendor,
        task: Task,
        installed: Option<Installed>,
    ) -> Result<Option<SigninStart>, SigninError> {
        let home = self
            .agents
            .get(agent)
            .and_then(|known| known.home.clone())
            .ok_or_else(|| SigninError::NoHome(agent.clone()))?;
        if installed.is_none() && !task.queues() {
            return Err(SigninError::NotReady(agent.clone()));
        }
        let mut inner = self.lock();
        if let Some(running) = inner.jobs.get(agent) {
            if !task.supersedes() {
                return Ok(None);
            }
            running.ticket.cancel.cancel();
        }
        inner.next_job += 1;
        let ticket = Ticket {
            job: inner.next_job,
            agent: agent.clone(),
            cancel: Arc::new(Cancel::new()),
        };
        let (sender, receiver) = if task.signs_in() {
            let (sender, receiver) = async_channel::unbounded();
            (Some(sender), Some(receiver))
        } else {
            (None, None)
        };
        let (kick, kicked) = if task.queues() {
            let (kick, kicked) = async_channel::bounded(1);
            (Some(kick), Some(kicked))
        } else {
            (None, None)
        };
        inner.jobs.insert(
            agent.clone(),
            Slot {
                ticket: ticket.clone(),
                task,
                paste: sender,
                kick,
            },
        );
        Ok(Some(SigninStart {
            ticket,
            task,
            vendor,
            installed,
            home,
            paste: receiver,
            kick: kicked,
        }))
    }

    /// `agent`'s install is `Ready`: a queued sign-in waiting for it looks again. A kick
    /// already waiting is enough, so a full channel drops this one.
    pub(crate) fn install_ready(&self, agent: &AgentId) {
        if let Some(kick) = self
            .lock()
            .jobs
            .get(agent)
            .and_then(|slot| slot.kick.as_ref())
        {
            // Full means a kick already waits; closed means the job is gone.
            let _sent = kick.try_send(());
        }
    }

    /// Waits for, then holds, the lock on Codex's login files ([`SigninHub::codex_files`]'s
    /// field docs).
    pub(crate) async fn codex_files(&self) -> futures::lock::MutexGuard<'_, ()> {
        self.codex_files.lock().await
    }

    /// Whether `ticket`'s job is still its agent's running job, and not cancelled: the check
    /// made right before a step that cannot wait for the job's end (a link, a record).
    pub(crate) fn is_live(&self, ticket: &Ticket) -> bool {
        !ticket.cancel.is_cancelled() && self.is_job(ticket)
    }

    /// Whether `ticket`'s job is still its agent's running job.
    pub(crate) fn is_job(&self, ticket: &Ticket) -> bool {
        is_current(&self.lock(), ticket)
    }

    /// Records and sends `state` for `ticket`'s job, unless it was superseded: then nothing
    /// happens and `false` comes back. `ends` marks its last state.
    pub(crate) fn emit_job(&self, ticket: &Ticket, state: SigninState, ends: bool) -> bool {
        let mut inner = self.lock();
        if !is_current(&inner, ticket) {
            return false;
        }
        if ends {
            inner.jobs.remove(&ticket.agent);
        }
        inner.states.insert(ticket.agent.clone(), state.clone());
        // Fails only when every receiver is gone; nobody is listening then.
        drop(self.events.try_send(AgentsEvent::Signin {
            agent: ticket.agent.clone(),
            state,
        }));
        true
    }

    /// Ends `ticket`'s job with the state unchanged.
    pub(crate) fn end_job(&self, ticket: &Ticket) {
        let mut inner = self.lock();
        if is_current(&inner, ticket) {
            inner.jobs.remove(&ticket.agent);
        }
    }

    /// Sends `note` for `ticket`'s job, unless it was superseded.
    pub(crate) fn emit_note(&self, ticket: &Ticket, note: SigninNote) {
        let inner = self.lock();
        if is_current(&inner, ticket) {
            drop(self.events.try_send(AgentsEvent::SigninNote {
                agent: ticket.agent.clone(),
                note,
            }));
        }
    }

    /// Sends `OpenUrl` for `ticket`'s job, unless it was superseded or cancelled.
    pub(crate) fn emit_open_url(&self, ticket: &Ticket, url: String) {
        let inner = self.lock();
        if is_current(&inner, ticket) && !ticket.cancel.is_cancelled() {
            drop(self.events.try_send(AgentsEvent::OpenUrl {
                agent: ticket.agent.clone(),
                url,
            }));
        }
    }

    /// The source of `agent`'s login while it is signed in.
    pub(crate) fn source(&self, agent: &AgentId) -> Option<Source> {
        if let Some(SigninState::SignedIn { source, .. }) = self.lock().states.get(agent) {
            Some(*source)
        } else {
            None
        }
    }

    /// Stops every job: the handle is gone.
    pub(super) fn stop_all(&self) {
        let jobs = std::mem::take(&mut self.lock().jobs);
        for slot in jobs.into_values() {
            slot.ticket.cancel.cancel();
        }
    }
}

fn is_current(inner: &Inner, ticket: &Ticket) -> bool {
    inner
        .jobs
        .get(&ticket.agent)
        .is_some_and(|slot| slot.ticket.job == ticket.job)
}

/// Starts a reserved job on its own thread, unless it was superseded meanwhile. A Claude sign-in
/// on macOS and Linux first (re)writes the `BROWSER` shim, here on the one worker thread, so two
/// jobs never write it at once.
pub(super) fn start_job<T: Timer + Send + Sync + 'static>(worker: &Worker<T>, start: SigninStart) {
    let SigninStart {
        ticket,
        task,
        vendor,
        installed,
        home,
        paste,
        kick,
    } = start;
    let hub = &worker.shared.signin;
    if !hub.is_job(&ticket) {
        return;
    }
    let fail = |ticket: &Ticket| {
        let state = SigninState::Failed {
            code: SigninCode::Failed,
            at: worker.config.clock.now(),
        };
        hub.emit_job(ticket, state, true);
    };
    if !worker.config.data_dir.is_absolute() {
        fail(&ticket);
        return;
    }
    let needs_shim =
        matches!(task, Task::SignIn(_) | Task::UseDifferent) && vendor == Vendor::Claude;
    let browser = if needs_shim {
        let Ok(browser) = shim_script(&worker.config) else {
            fail(&ticket);
            return;
        };
        browser
    } else {
        None
    };
    let job = SigninJob {
        ticket: ticket.clone(),
        task,
        vendor,
        config: worker.config.clone(),
        installed,
        home,
        browser,
        launcher: Arc::clone(&worker.signin_launcher),
        timer: Arc::clone(&worker.timer),
        shared: Arc::clone(&worker.shared),
        paste,
    };
    let spawned = worker
        .shared
        .spawn_job(format!("clauseo-signin-{}", ticket.agent), move || {
            job::run(job, kick);
        });
    if spawned.is_err() {
        fail(&ticket);
    }
}

/// The shim on macOS and Linux.
#[cfg(unix)]
fn shim_script(config: &AgentsConfig) -> std::io::Result<Option<PathBuf>> {
    crate::signin::shim::install(&config.data_dir).map(Some)
}

/// None on Windows, where Claude opens its own tab.
#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the Unix version's signature, which can fail; one call site serves both"
)]
fn shim_script(config: &AgentsConfig) -> std::io::Result<Option<PathBuf>> {
    let _ = config;
    Ok(None)
}
