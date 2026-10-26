//! What the host passes in and gets back from [`AcpClient`](crate::AcpClient)'s calls.

use std::path::PathBuf;

use clauseo_protocol::{
    AgentCapabilities, AgentSessionId, CompactString, ConfigOption, StopReason,
};

use crate::event::{LimitSignal, RawFrame};

/// Options for [`connect`](crate::connect).
#[derive(Debug, Clone, Default)]
pub struct ConnectOptions {
    /// Receives a copy of every line in and out, with `try_send`: when the channel is full the
    /// frame is dropped and counted ([`AcpClient::frames_dropped`](crate::AcpClient::frames_dropped)).
    pub frame_tap: Option<async_channel::Sender<RawFrame>>,
}

/// What `initialize` told about the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInfo {
    /// `agentInfo.name`, or empty when the agent sent none.
    pub name: String,
    /// `agentInfo.version`.
    pub version: Option<String>,
    /// What it supports. `steering` stays `None` until steer is built.
    pub capabilities: AgentCapabilities,
    /// How it signs in, in its order.
    pub auth_methods: Vec<AuthMethod>,
}

/// One way the agent signs in. Stays an `acp` type until a host command needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthMethod {
    /// The id `authenticate` takes.
    pub id: String,
    /// Its name, for display.
    pub name: String,
    /// Its description.
    pub description: Option<String>,
    /// Whether the agent signs in by itself or in a terminal.
    pub kind: AuthKind,
}

/// How an [`AuthMethod`] runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthKind {
    /// The agent signs in by itself when `authenticate` names it.
    Agent,
    /// The client runs the agent in a terminal with these arguments and variables; never through
    /// `authenticate` (stage D's `agents` runs it).
    Terminal {
        /// The extra arguments.
        args: Vec<String>,
        /// The extra environment variables, sorted by name.
        env: Vec<(String, String)>,
    },
}

/// `session/new`'s parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct NewSession {
    /// The session's folder; tool locations inside it become matter paths.
    pub cwd: PathBuf,
    /// The MCP servers; always sent, empty in the first app.
    pub mcp_servers: Vec<McpServer>,
    /// The `_meta` object, sent verbatim and never read.
    pub meta: Option<serde_json::Map<String, serde_json::Value>>,
}

/// `session/load`'s parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadSession {
    /// The agent's session to resume.
    pub session: AgentSessionId,
    /// The session's folder.
    pub cwd: PathBuf,
    /// The MCP servers; always sent, empty in the first app.
    pub mcp_servers: Vec<McpServer>,
}

/// An MCP server the agent starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServer {
    /// Over stdio, the transport every agent must support.
    Stdio {
        /// Its name.
        name: String,
        /// The program.
        command: PathBuf,
        /// Its arguments.
        args: Vec<String>,
        /// Its environment variables.
        env: Vec<(String, String)>,
    },
}

/// What `session/new` and `session/load` gave back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStart {
    /// The agent's session id.
    pub session: AgentSessionId,
    /// The modes, when the agent has any.
    pub modes: Option<SessionModes>,
    /// The config options, empty when the agent sent none.
    pub config_options: Vec<ConfigOption>,
}

/// The agent's modes and the current one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionModes {
    /// The current mode's id.
    pub current: CompactString,
    /// Every mode, in the agent's order.
    pub available: Vec<SessionMode>,
}

/// One mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMode {
    /// Its id, which `set_mode` takes.
    pub id: CompactString,
    /// Its name.
    pub name: String,
    /// Its description.
    pub description: Option<String>,
}

/// How a prompt ended.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptEnd {
    /// The agent's stop reason.
    pub stop: StopReason,
    /// A plan limit the response carried (an AIR `sessionFailure` in its `_meta`).
    pub limit: Option<LimitSignal>,
}

/// What `steer()` did. Steer is not built yet: `steer()` returns `Error::SteerUnsupported`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SteerResult {
    /// The text joined the running turn.
    Injected,
    /// The turn had just ended and the agent sent nothing; send the text as a normal prompt.
    PromptRequired,
    /// The agent started a new turn with it.
    StartedNewTurn,
    /// The agent refused it, or answered with an error or an outcome the driver does not know.
    Failed,
}
