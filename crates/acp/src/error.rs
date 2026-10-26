//! [`Error`]: every failure of a call or of the driver, each with its `protocol` code.

use clauseo_protocol::ErrorCode;

use crate::types::AuthMethod;

/// Why a call or the driver failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The connection is closed: the agent exited, or the driver ended.
    #[error("the agent connection is closed")]
    Closed,
    /// The agent answered `initialize` with a protocol version other than 1; the connection is
    /// closed.
    #[error("the agent speaks ACP version {agent}, not 1")]
    UnsupportedVersion {
        /// The version it answered.
        agent: u16,
    },
    /// The agent needs `authenticate` first (JSON-RPC error -32000).
    #[error("the agent needs authentication")]
    AuthRequired {
        /// The ways it signs in, from `initialize`.
        methods: Vec<AuthMethod>,
    },
    /// The agent answered with a JSON-RPC error.
    #[error("the agent answered with error {code}: {message}")]
    Rpc {
        /// The JSON-RPC code.
        code: i32,
        /// Its message.
        message: String,
    },
    /// The agent broke the protocol: a line over 8 MiB, or an answer the driver cannot use.
    #[error("the agent broke the protocol: {0}")]
    Protocol(String),
    /// A prompt is in flight on this session; nothing was sent.
    #[error("a prompt is already running in this session")]
    TurnRunning,
    /// No permission request is pending under this key.
    #[error("no permission request is pending under this key")]
    PermissionNotPending,
    /// The agent did not offer this option; the request stays pending.
    #[error("the agent did not offer this option")]
    UnknownOption,
    /// No elicitation is pending under this key.
    #[error("no elicitation is pending under this key")]
    ElicitationNotPending,
    /// The agent does not accept this kind of prompt content.
    #[error("the agent does not accept this prompt content")]
    UnsupportedContent,
    /// No session with this id was started or loaded on this connection.
    #[error("no such session on this connection")]
    UnknownSession,
    /// `authenticate` named a terminal method, which the client must run itself.
    #[error("a terminal sign-in method cannot be run through authenticate")]
    TerminalAuthMethod,
    /// The agent cannot steer a running turn safely.
    #[error("the agent cannot steer a running turn")]
    SteerUnsupported,
}

impl Error {
    /// The code a client sees.
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Closed => ErrorCode::AgentExited,
            Self::UnsupportedVersion { .. } => ErrorCode::AgentUnsupportedVersion,
            Self::AuthRequired { .. } => ErrorCode::AgentAuthRequired,
            Self::Rpc { .. } | Self::Protocol(_) => ErrorCode::AgentProtocolError,
            Self::TurnRunning => ErrorCode::ThreadTurnRunning,
            Self::PermissionNotPending => ErrorCode::PermissionNotPending,
            Self::UnknownOption => ErrorCode::PermissionUnknownOption,
            Self::ElicitationNotPending => ErrorCode::ElicitationNotPending,
            Self::UnsupportedContent => ErrorCode::CommandInvalid,
            Self::UnknownSession | Self::TerminalAuthMethod | Self::SteerUnsupported => {
                ErrorCode::Internal
            }
        }
    }
}
