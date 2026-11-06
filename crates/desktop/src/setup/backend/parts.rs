//! Small parts of the backend: first run's decision, the plan login a `StartSignIn` asks for,
//! a sign-in call the identity check expects, the endpoint check before `SetFirmEndpoint`'s
//! confirm, and its two error answers.

use std::collections::BTreeSet;

use clauseo_agents::{InstallState, Method, SigninError};
use clauseo_protocol::{AgentId, AgentKind, ErrorCode, HostError};

use crate::setup::identity::Identities;

/// Which first install states have come in: first run is decided once every agent has one.
pub(super) struct FirstRun {
    waiting: BTreeSet<AgentId>,
    /// Whether an agent's first state was `Ready` or `Managed`.
    any_present: bool,
    decided: bool,
}

impl FirstRun {
    pub(super) fn new(agents: impl IntoIterator<Item = AgentId>) -> Self {
        Self {
            waiting: agents.into_iter().collect(),
            any_present: false,
            decided: false,
        }
    }

    /// Notes `agent`'s `state`; `true` once, when the last first state came in and none was
    /// `Ready` or `Managed`: first run.
    pub(super) fn saw(&mut self, agent: &AgentId, state: &InstallState) -> bool {
        if self.decided || !self.waiting.remove(agent) {
            return false;
        }
        self.any_present |= matches!(state, InstallState::Ready { .. } | InstallState::Managed);
        if !self.waiting.is_empty() {
            return false;
        }
        self.decided = true;
        !self.any_present
    }
}

/// The plan login `StartSignIn` asks for: Claude's Claude.ai plan or, with `console`, its
/// Console account; Codex's browser sign-in (`console` is Claude's alone).
pub(super) fn plan_method(agent: &AgentId, console: bool) -> Result<Method, HostError> {
    match (AgentKind::of(agent), console) {
        (Some(AgentKind::Claude), false) => Ok(Method::ClaudeAi),
        (Some(AgentKind::Claude), true) => Ok(Method::Console),
        (Some(AgentKind::Codex), false) => Ok(Method::Browser),
        _ => Err(invalid(format!("{agent} has no such sign-in"))),
    }
}

/// `call`, an S18 call that signs `agent` in through Clauseo (a sign-in, a reuse): the identity
/// check expects it first, so its `SignedIn` records the account before anything compares it
/// (R17), and stops expecting it when S18 refuses it.
pub(super) fn signs_in(
    identity: &Identities,
    agent: &AgentId,
    call: impl FnOnce() -> Result<(), SigninError>,
) -> Result<(), HostError> {
    identity.expect(agent);
    call().map_err(|error| {
        identity.expect_none(agent);
        signin_error(&error)
    })
}

/// The endpoint `url` as the confirm shows it and the plan stores it: an address the settings
/// crate takes (`web_address_ok`: ASCII only, no space, line break, control character, `?`,
/// `#`, `@` or backslash) that parses as an `https` URL with a host and no user name or
/// password, else `command.invalid` before any confirm. The message never repeats it.
pub(super) fn checked_endpoint(url: &str) -> Result<String, HostError> {
    if !clauseo_settings::web_address_ok(url) {
        return Err(invalid(
            "the endpoint must be an https address of plain characters, with no query or fragment",
        ));
    }
    let parsed = url::Url::parse(url).map_err(|_error| invalid("the endpoint is not a URL"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(invalid(
            "the endpoint must be an https address with no user name or password",
        ));
    }
    Ok(url.to_owned())
}

/// The answer to a refused S18 call: `command.invalid`, except a missing home (`internal`).
/// The text names the agent, never a code or an email.
pub(crate) fn signin_error(error: &SigninError) -> HostError {
    let code = match error {
        SigninError::NoHome(_) => ErrorCode::Internal,
        _ => ErrorCode::CommandInvalid,
    };
    HostError::new(code, error.to_string())
}

/// A `command.invalid` failure with `message`.
pub(super) fn invalid(message: impl Into<String>) -> HostError {
    HostError::new(ErrorCode::CommandInvalid, message)
}
