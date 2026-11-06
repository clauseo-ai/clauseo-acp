//! The host calls `RecordingHost` records, as tests compare them.

use clauseo_host::StopFor;
use clauseo_protocol::{AgentId, ErrorCode};
use clauseo_settings::AgentLaunch;

/// One host call as `RecordingHost` saw it.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::setup::backend) enum HostCall {
    SetAgentLaunch(AgentId, AgentLaunch),
    SetEndpoint(AgentId, Option<String>),
    RestartAgents(AgentId),
    StopAgents(AgentId),
    StopAgentsFor(AgentId, StopFor),
    SetReusedLogin(AgentId, bool),
    RemoveLoginLinks(AgentId),
    LoginChanged(AgentId),
    SetLaunchRefused(AgentId, Option<ErrorCode>),
}

impl std::fmt::Debug for HostCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetAgentLaunch(agent, launch) => {
                write!(f, "set_agent_launch({agent}, {launch:?})")
            }
            Self::SetEndpoint(agent, url) => write!(f, "set_endpoint({agent}, {url:?})"),
            Self::RestartAgents(agent) => write!(f, "restart_agents({agent})"),
            Self::StopAgents(agent) => write!(f, "stop_agents({agent})"),
            Self::StopAgentsFor(agent, why) => write!(f, "stop_agents_for({agent}, {why:?})"),
            Self::SetReusedLogin(agent, reused) => write!(f, "set_reused_login({agent}, {reused})"),
            Self::RemoveLoginLinks(agent) => write!(f, "remove_login_links({agent})"),
            Self::LoginChanged(agent) => write!(f, "login_changed({agent})"),
            Self::SetLaunchRefused(agent, code) => {
                write!(f, "set_launch_refused({agent}, {code:?})")
            }
        }
    }
}
