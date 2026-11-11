//! What the host passes in and gets back from [`AcpClient`](crate::AcpClient)'s calls.

use std::path::PathBuf;

use clauseo_protocol::{
    AgentCapabilities, AgentSessionId, CompactString, ConfigOption, StopReason,
};

use crate::event::{ConnectionLost, LimitSignal, RawFrame};

/// Options for [`connect`](crate::connect).
#[derive(Debug, Clone, Default)]
pub struct ConnectOptions {
    /// Receives a copy of every line in and out, with `try_send`: when the channel is full the
    /// frame is dropped and counted, as
    /// [`AcpClient::frames_dropped`](crate::AcpClient::frames_dropped) tells.
    pub frame_tap: Option<async_channel::Sender<RawFrame>>,
}

/// What `initialize` told about the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInfo {
    /// `agentInfo.name`; `None` when the agent sent no `agentInfo`.
    pub name: Option<String>,
    /// `agentInfo.version`; `None` when the agent sent no `agentInfo`.
    pub version: Option<String>,
    /// `agentInfo.title`; `None` when the agent sent no `agentInfo` or no title.
    pub title: Option<String>,
    /// What it supports. `steering` is `Some` when the agent advertises native steering.
    pub capabilities: AgentCapabilities,
    /// How it signs in, in its order.
    pub auth_methods: Vec<AuthMethod>,
    /// `agentCapabilities.mcpCapabilities.http`: whether it takes an http MCP server. To one
    /// that does not, [`McpServer::Http`] is left out of every session call.
    pub mcp_http: bool,
}

/// One way the agent signs in. Stays an `acp` type until a host command needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthMethod {
    /// The id `authenticate` takes.
    pub id: CompactString,
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
    /// ACP's `additionalDirectories`: more workspace roots, absolute; not sent when empty.
    pub additional_directories: Vec<PathBuf>,
    /// The MCP servers; always sent, empty when none is turned on.
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
    /// ACP's `additionalDirectories`, as `session/new` sent them: on load and resume the list
    /// is the session's whole set of extra roots.
    pub additional_directories: Vec<PathBuf>,
    /// The MCP servers; always sent, empty when none is turned on.
    pub mcp_servers: Vec<McpServer>,
    /// The `_meta` object, sent verbatim and never read. An adapter may read its options again
    /// on load (claude-agent-acp does), so send what `session/new` sent.
    pub meta: Option<serde_json::Map<String, serde_json::Value>>,
}

/// An MCP server the agent starts or reaches. A stdio server goes on the wire with no `type`
/// field, as ACP v1 has it: claude-agent-acp 0.89.1 drops a stdio server sent with
/// `"type": "stdio"`. An http server goes as `{"type": "http", name, url, headers}`, ACP v1's
/// `McpServerHttp`, and only to an agent whose `initialize` said `mcpCapabilities.http`; to
/// any other it is left out and its name logged. There is no sse variant: codex-acp 2.2.2 has
/// none.
///
/// `Debug` leaves out env and header values and the URL, which can hold secrets, and prints
/// their names. A stdio server's command and args are printed as they are: put a secret in its
/// env, not its args.
#[derive(Clone, PartialEq, Eq)]
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
    /// Over streamable HTTP, which the agent's own MCP client reaches.
    Http {
        /// Its name.
        name: String,
        /// Its URL.
        url: String,
        /// The HTTP headers the agent sends with each request, as name and value.
        headers: Vec<(String, String)>,
    },
}

impl McpServer {
    /// Its name, whichever transport.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Stdio { name, .. } | Self::Http { name, .. } => name,
        }
    }
}

impl std::fmt::Debug for McpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names = |pairs: &[(String, String)]| {
            pairs
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
        };
        match self {
            Self::Stdio {
                name,
                command,
                args,
                env,
            } => f
                .debug_struct("Stdio")
                .field("name", name)
                .field("command", command)
                .field("args", args)
                .field("env_names", &names(env))
                .finish(),
            Self::Http { name, headers, .. } => f
                .debug_struct("Http")
                .field("name", name)
                .field("header_names", &names(headers))
                .finish_non_exhaustive(),
        }
    }
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
    /// The response's AIR `sessionFailure` says the model refused the turn as too long to
    /// read, and how (`map/too_long.rs`). Never a plan limit: `limit` stays `None` for it.
    pub too_long: Option<TooLong>,
    /// A lost connection the response carried (an AIR `sessionFailure` of category
    /// `connection` in its `_meta`): codex-acp ends a turn whose retries ran out so.
    pub connection: Option<ConnectionLost>,
}

/// How an AIR `sessionFailure` said a turn was too long to read. The driver does not know the
/// agent; the host decides by the agent's kind which of these ends a turn `TooLong`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TooLong {
    /// Titled "Prompt is too long" (Claude Code's own words): the question, or the conversation
    /// with it, is over the model's context.
    PromptTooLong,
    /// Category `limit`, actions `["new_session"]`: codex-acp's `context_exhausted`, a full
    /// context. claude-agent-acp sends the same shape for its configured turn or output limit,
    /// which is not "too long".
    ContextExhausted,
}

/// What `steer()` did. `steer()` gives `Error::SteerUnsupported`, sending nothing, unless the
/// agent steers natively and falls back to the host when idle.
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
