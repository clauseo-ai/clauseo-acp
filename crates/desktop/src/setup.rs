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
//! - **A key from `main`** goes to the backend as [`KeyWrite::OnlyIfAbsent`]: the backend
//!   checks that no key is stored and stores it in one step, and refuses with
//!   `settings.key_stored` otherwise, so two quick sends cannot both pass.
//! - **Secrets.** A pasted code or a key is a `Secret`, moved into the backend with its
//!   command and dropped there. Logs name the window, the action, the agent and, on failure,
//!   the code; never a field of the command.
//!
//! Until R20 P5b puts S18's `Agents` and the host's Rust-only calls behind the same trait, the
//! app runs [`NotWired`], which answers every command with `internal` and publishes only
//! `setup_info` with no keys,
//! and the tests a scripted `StubBackend` (`stub.rs`).

mod admit;
mod events;
mod stub;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use clauseo_protocol::{AgentId, CommandOutcome, HostError, SetupAction, SetupCommand, SetupEvent};

use crate::bridge::lock;
use crate::dialogs::BoxFuture;

pub(crate) use admit::may_setup;
pub(crate) use events::SetupEvents;
pub(crate) use stub::NotWired;
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
}

/// The `setup` command's path: the allow-lists, the sign-in join, the backend and its events.
pub(crate) struct Setup {
    backend: Arc<dyn SetupBackend>,
    events: Arc<SetupEvents>,
    /// The agents whose `StartSignIn` is with the backend now.
    starting: Mutex<BTreeSet<AgentId>>,
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
            starting: Mutex::new(BTreeSet::new()),
            main_sets_first_key,
        }
    }

    /// Setup run by [`NotWired`], the app's until R20 P5b. Its `setup_info` says no key can
    /// be kept, so the windows hide the key screens until a backend can keep one.
    pub(crate) fn not_wired() -> Self {
        let setup = Self::new(Arc::new(NotWired));
        setup.events.publish(SetupEvent::SetupInfo {
            keys_available: false,
        });
        setup
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
        let _starting = if action == SetupAction::StartSignIn {
            let Some(starting) = self.start_sign_in(&agent) else {
                log_answer(label, action, &agent, "joined", None);
                return Ok(CommandOutcome::Done);
            };
            Some(starting)
        } else {
            None
        };
        let answer = self.backend.run(command, key_write, &self.events).await;
        match &answer {
            Ok(_) => log_answer(label, action, &agent, "done", None),
            Err(error) => log_answer(label, action, &agent, "failed", Some(error)),
        }
        answer
    }

    /// Marks `agent`'s sign-in as starting, unless one runs or is starting: then `None`, and
    /// the caller joins it. The mark goes when the guard drops, the backend having answered
    /// (and published `Queued` or the like) or the call having been dropped.
    fn start_sign_in(&self, agent: &AgentId) -> Option<Starting<'_>> {
        let mut starting = lock(&self.starting);
        if starting.contains(agent) || self.events.sign_in_running(agent) {
            return None;
        }
        starting.insert(agent.clone());
        Some(Starting {
            setup: self,
            agent: agent.clone(),
        })
    }
}

/// An agent's `StartSignIn` with the backend; dropped, it no longer is.
struct Starting<'a> {
    setup: &'a Setup,
    agent: AgentId,
}

impl Drop for Starting<'_> {
    fn drop(&mut self) {
        lock(&self.setup.starting).remove(&self.agent);
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
