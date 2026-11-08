//! Small parts of the backend: the plan login a `StartSignIn` asks for, the endpoint check
//! before `SetFirmEndpoint`'s confirm, and its two error answers; a retry's check, and the wait
//! for the first network route before a command that reaches the network.

use clauseo_agents::{InstallState, Method, SigninError};
use clauseo_protocol::{AgentId, AgentKind, ErrorCode, HostError, SetupCommand};

use super::AgentsBackend;

/// `RetryAgentInstall`: `command.invalid` unless `agent`'s install is `Failed`, `Waiting` or
/// `Paused`; a missing agent installs through `InstallAgent` or a sign-in press.
pub(super) fn retry(
    backend: &AgentsBackend,
    agent: &AgentId,
    use_system_proxy: bool,
) -> Result<(), HostError> {
    let restartable = matches!(
        backend.agents.state(agent),
        Some(InstallState::Failed(_) | InstallState::Waiting { .. } | InstallState::Paused)
    );
    if !restartable {
        return Err(invalid(format!(
            "{agent}'s install is not failed, waiting or paused; choose the agent to install it"
        )));
    }
    backend.agents.retry(agent, use_system_proxy);
    Ok(())
}

/// Before an install, a retry or a sign-in: waits for the first network route to reach
/// `Agents` (`network/follow.rs`), so the first try goes through the resolved proxy and
/// never direct first.
pub(super) async fn network_first(backend: &AgentsBackend, command: &SetupCommand) {
    let reaches = matches!(
        command,
        SetupCommand::InstallAgent { .. }
            | SetupCommand::RetryAgentInstall { .. }
            | SetupCommand::StartSignIn { .. }
            | SetupCommand::UseDeviceCode { .. }
            | SetupCommand::UseDifferentAccount { .. }
    );
    if reaches {
        backend.wiring.network.ready().await;
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
