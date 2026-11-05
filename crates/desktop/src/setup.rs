//! Sign-in and installs from the windows (decision record 0048): what the `setup` command runs.
//! A window's [`SetupCommand`] is admitted by its label first (`admit.rs`: one table of which
//! window may send which [`SetupAction`]), then goes to the [`SetupBackend`], which does the
//! work and publishes what changed as [`SetupEvent`]s on [`SetupEvents`]. Each window that
//! follows them (`main` and `settings`, from `subscribe_app` on) gets them on its channel in a
//! `setup` message, the latest state first (`events.rs`).
//!
//! - **A second `StartSignIn`** for an agent whose sign-in runs (`Queued`, `Waiting`,
//!   `DeviceCode`), or is being started by another call, answers `Done` and joins it: the
//!   backend is not called again. S18's engine restarts a running job on a new sign-in, so the
//!   join is desktop's, here, whatever backend runs.
//! - **A bar replaces the join.** While a firm policy sets the agent's endpoint, a key is
//!   stored or the lawyer set an endpoint, a sign-in start is refused from every window
//!   (`signin.firm_managed`, `signin.key_in_use`, `signin.endpoint_in_use`, in that order of
//!   precedence; `admit::sign_in_bar`), before any join, and a running sign-in is ended when
//!   one of them takes over. The `starting` mark holds until a sign-in state is published for
//!   the agent, S18 reports a resting one, or a key or an endpoint cancels the sign-in, so two
//!   quick starts never reach the backend twice and a start that ended unreported never sticks.
//! - **A key from `main`** goes to the backend as [`KeyWrite::OnlyIfAbsent`]: the backend
//!   checks that no key is stored and stores it in one step, and refuses with
//!   `settings.key_stored` otherwise, so two quick sends cannot both pass.
//! - **A failed keychain read refuses and never writes.** A read that fails (a locked keychain,
//!   a denied prompt, an item that is not a key) is not "absent": an `OnlyIfAbsent` key is
//!   refused (`internal`, `Keychain::store_if_absent` passing the read's error up) and nothing
//!   is stored. `settings`' `Replace` writes without reading, so it may replace a key; `main`
//!   may only store one when none is there.
//! - **Secrets.** A pasted code or a key is a `Secret`, moved into the backend with its
//!   command and dropped there. Logs name the window, the action, the agent and, on failure,
//!   the code; never a field of the command.
//!
//! The app's backend is [`backend::AgentsBackend`], over S18's `Agents`, the keychain and the
//! host's Rust-only calls (`backend.rs`, `keys.rs`, `compose.rs`, `host_calls.rs`), behind a
//! cell its start fills (`start.rs`); the tests use a scripted `StubBackend` (`stub.rs`) too.

mod admit;
mod backend;
mod compose;
mod confirm;
mod events;
mod firm_policy;
mod host_calls;
mod keys;
mod start;
#[cfg(test)]
mod stub;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use clauseo_protocol::{AgentId, CommandOutcome, HostError, SetupAction, SetupCommand};

use crate::dialogs::BoxFuture;

pub(crate) use admit::may_setup;
pub(crate) use confirm::NativeSetupConfirm;
pub(crate) use events::SetupEvents;
pub(crate) use host_calls::CellHost;
pub(crate) use start::{BackendCell, SetupParts, SetupStart, spawn};
#[cfg(test)]
pub(crate) use stub::StubBackend;

/// How a `SetAgentKey` may store its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyWrite {
    /// Store it, replacing a key already stored: from the settings window.
    Replace,
    /// Store it only when the agent has no key, in the same step as the check, else refuse
    /// with `admit::key_stored`: from `main` (its first-run "Use an API key").
    OnlyIfAbsent,
}

/// What does the work of a setup command: installs, sign-ins, keys and endpoints.
pub(crate) trait SetupBackend: Send + Sync {
    /// Runs `command`, which the window was allowed to send, and publishes each state it
    /// changes on `events`, in order. `key_write` says how a `SetAgentKey` stores; other
    /// commands ignore it. The command, and any `Secret` in it, is dropped when the call ends.
    fn run<'a>(
        &'a self,
        command: SetupCommand,
        key_write: KeyWrite,
        events: &'a SetupEvents,
    ) -> BoxFuture<'a, Result<CommandOutcome, HostError>>;

    /// Why `agent`'s sign-in may not start now, if anything bars it: a firm policy's endpoint,
    /// a stored key, an endpoint the lawyer set (`admit::sign_in_bar`). Asked before a start
    /// joins a running sign-in, since a bar replaces the join. `None` by default.
    fn sign_in_refused(&self, _agent: &AgentId) -> Option<HostError> {
        None
    }
}

/// The `setup` command's path: the allow-lists, the sign-in join, the backend and its events.
pub(crate) struct Setup {
    backend: Arc<dyn SetupBackend>,
    events: Arc<SetupEvents>,
    /// Whether `main` may send `SetAgentKey` (only while no key is stored).
    main_sets_first_key: bool,
}

impl Setup {
    /// Setup run by `backend`, with the allow-lists as 0048 and its rulings set them.
    pub(crate) fn new(backend: Arc<dyn SetupBackend>) -> Self {
        Self::with_rules(backend, admit::MAIN_SETS_FIRST_KEY)
    }

    /// [`Setup::new`] with `main`'s first key allowed or not.
    pub(crate) fn with_rules(backend: Arc<dyn SetupBackend>, main_sets_first_key: bool) -> Self {
        Self {
            backend,
            events: Arc::new(SetupEvents::default()),
            main_sets_first_key,
        }
    }

    /// Tests: [`Setup::new`] publishing on `events`, which the backend also holds.
    #[cfg(test)]
    pub(crate) fn with_events(backend: Arc<dyn SetupBackend>, events: Arc<SetupEvents>) -> Self {
        Self {
            events,
            ..Self::new(backend)
        }
    }

    /// Where the backend publishes, and the windows' channels follow.
    pub(crate) fn events(&self) -> Arc<SetupEvents> {
        Arc::clone(&self.events)
    }

    /// Runs `command` from the window `label`: refused with `transport.unauthorized` before
    /// anything runs when the window may not send it; `Done` with nothing run for a
    /// `StartSignIn` that joins a running sign-in; else the backend's answer.
    pub(crate) async fn run(
        &self,
        label: &str,
        command: SetupCommand,
    ) -> Result<CommandOutcome, HostError> {
        let action = command.action();
        let agent = command.agent().clone();
        let key_write = match admit::admit(self.main_sets_first_key, label, action) {
            Ok(key_write) => key_write,
            Err(error) => {
                log_answer(label, action, &agent, "refused", Some(&error));
                return Err(error);
            }
        };
        if starts_sign_in(action)
            && let Some(error) = self.backend.sign_in_refused(&agent)
        {
            log_answer(label, action, &agent, "refused", Some(&error));
            return Err(error);
        }
        let starting = if action == SetupAction::StartSignIn {
            let Some(starting) = self.start_sign_in(&agent) else {
                log_answer(label, action, &agent, "joined", None);
                return Ok(CommandOutcome::Done);
            };
            Some(starting)
        } else {
            None
        };
        let answer = self.backend.run(command, key_write, &self.events).await;
        if let Some(starting) = starting
            && answer.is_ok()
        {
            starting.keep();
        }
        match &answer {
            Ok(_) => log_answer(label, action, &agent, "done", None),
            Err(error) => log_answer(label, action, &agent, "failed", Some(error)),
        }
        answer
    }

    /// Marks `agent`'s sign-in as starting, unless one runs or is starting: then `None`, and
    /// the caller joins it. The mark is [`SetupEvents`]', so it holds past the backend's answer
    /// until a sign-in state is published for `agent`, S18 reports a resting one, or a key or
    /// an endpoint cancels the sign-in: S18 reports `Queued` or `Waiting` later, through its
    /// worker and the pump, and a second start in between would restart the run. It goes at
    /// once when the backend refuses, or the call is dropped, unless [`Starting::keep`] was
    /// called.
    fn start_sign_in(&self, agent: &AgentId) -> Option<Starting<'_>> {
        self.events.begin_start(agent).then(|| Starting {
            events: &self.events,
            agent: agent.clone(),
            keep: false,
        })
    }
}

/// Whether `action` starts an S18 sign-in run, which a key or an endpoint bars.
fn starts_sign_in(action: SetupAction) -> bool {
    matches!(
        action,
        SetupAction::StartSignIn
            | SetupAction::UseDeviceCode
            | SetupAction::UseFoundLogin
            | SetupAction::UseDifferentAccount
    )
}

/// An agent's `StartSignIn` with the backend; dropped, the mark goes unless kept.
struct Starting<'a> {
    events: &'a SetupEvents,
    agent: AgentId,
    keep: bool,
}

impl Starting<'_> {
    /// The backend took the start: the mark stays until a sign-in state is published, S18
    /// reports a resting one, or a key or an endpoint cancels the sign-in.
    fn keep(mut self) {
        self.keep = true;
    }
}

impl Drop for Starting<'_> {
    fn drop(&mut self) {
        if !self.keep {
            self.events.end_start(&self.agent);
        }
    }
}

/// Logs a setup command's answer: the window, the action, the agent and the code. Nothing of
/// the command's fields, nor the error's message.
fn log_answer(
    label: &str,
    action: SetupAction,
    agent: &AgentId,
    outcome: &str,
    error: Option<&HostError>,
) {
    let code = error.map(|error| error.code().as_str());
    tracing::info!(window = label, ?action, %agent, outcome, code, "setup");
}
