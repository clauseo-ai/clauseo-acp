//! The API-key `authenticate` retry (R20 brief, "API keys"; decision record 0048, P4b). A Codex
//! agent launched with a key (the profile hook's launch env sets [`KEY_VAR`]) that still
//! answers a session or prompt call with auth-required gets one `authenticate` with codex-acp's
//! `api-key` method, which reads the key from its own environment, and the call goes again.
//! Only when the launch's sign-in check answered [`LoginLink::Own`] and its `shared` answered
//! no (the `authenticate` guard, P4d): never while the login is
//! the lawyer's linked one (`Shared`, desktop says it is reused, a split, a gone file, or no
//! answer; S18's guard). One try per agent process: a second auth-required, or an
//! `authenticate` that fails, ends the call with the first auth-required, so the turn is held
//! for the sign-in as any other (`sign_in.rs`). A prompt's retry also carries its turn's Stop
//! ([`KeyRetry::unless_stopped`]): a Stop while `authenticate` runs found no prompt in flight, so
//! the agent dropped its `session/cancel`, and the prompt is not sent again; the first answer
//! stands and the turn ends cancelled. The host never reads the key's value; the logs name the
//! outcome and an error's code only.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::rc::Rc;

use clauseo_acp::AcpClient;
use clauseo_protocol::ThreadId;

use super::super::Actor;
use super::state;
use crate::agent::login_link::{Checked, LoginLink, Seen, treated_as_shared};

/// codex-acp's sign-in method that takes the key from its environment (research 22 §2.4).
const API_KEY_METHOD: &str = "api-key";

/// The variable a Codex key is launched in.
const KEY_VAR: &str = "CODEX_API_KEY";

/// A Codex agent's one retry.
#[derive(Debug, Clone, Default)]
pub(super) struct KeyRetry {
    /// `None` when it may not retry at all, else whether the try is left (shared by every call
    /// to the agent process).
    left: Option<Rc<Cell<bool>>>,
    /// The turn's Stop, for a prompt: once set, the call is not sent again.
    stopped: Option<Stopped>,
}

/// A turn's Stop, set by the Cancel path (`cancel.rs`) and read by its prompt's retry.
pub(super) type Stopped = Rc<Cell<bool>>;

impl KeyRetry {
    /// A launch's retry: armed when the profile hook's env (`hook`) sets a key and the launch's
    /// sign-in check (`login`) answered that the home's login is Clauseo's own and is not the
    /// lawyer's file (the guard's `shared`, `Unknown` counting as shared).
    pub(super) fn at_launch(hook: &BTreeMap<OsString, OsString>, login: Option<Checked>) -> Self {
        let key = hook
            .get(OsStr::new(KEY_VAR))
            .is_some_and(|value| !value.is_empty());
        let own = login.is_some_and(|checked| {
            checked.link == Seen::Answered(LoginLink::Own) && !treated_as_shared(checked.shared)
        });
        Self {
            left: (key && own).then(|| Rc::new(Cell::new(true))),
            stopped: None,
        }
    }

    /// The retry for a prompt of a turn whose Stop sets `stopped`.
    pub(super) fn unless_stopped(self, stopped: Stopped) -> Self {
        Self {
            stopped: Some(stopped),
            ..self
        }
    }

    /// Runs `call`; when it answers auth-required and the try is left, takes the try:
    /// `authenticate` with `api-key`, then `call` again. Otherwise, or when `authenticate`
    /// fails, or the turn was stopped meanwhile, the first answer stands.
    pub(super) async fn run<T, F, Fut>(
        &self,
        client: &AcpClient,
        call: F,
    ) -> Result<T, clauseo_acp::Error>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, clauseo_acp::Error>>,
    {
        let first = call().await;
        if !matches!(first, Err(clauseo_acp::Error::AuthRequired { .. })) {
            return first;
        }
        let Some(left) = &self.left else {
            return first;
        };
        if !left.replace(false) {
            return first;
        }
        tracing::info!("the agent asks for a sign-in with a key set: authenticate api-key, once");
        match client.authenticate(API_KEY_METHOD).await {
            Ok(()) if self.stopped.as_ref().is_some_and(|stopped| stopped.get()) => {
                tracing::info!("the turn was stopped during authenticate; the prompt is not sent");
                first
            }
            Ok(()) => call().await,
            Err(error) => {
                let code = error.code();
                tracing::warn!(%code, "authenticate api-key failed; the sign-in is asked for");
                first
            }
        }
    }
}

/// The retry for a call to `thread`'s agent about to go out: none while desktop says the login
/// is reused (a link exists), else what its launch armed.
pub(super) fn for_call(actor: &mut Actor, thread: ThreadId) -> KeyRetry {
    let reused = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| actor.wiring.sign_in.reused.contains(&entry.row.agent));
    if reused {
        return KeyRetry::default();
    }
    state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .and_then(|agent| agent.codex.as_ref())
        .map(|codex| codex.key_retry.clone())
        .unwrap_or_default()
}
