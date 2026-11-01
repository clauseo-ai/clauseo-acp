//! [`Error`]: every failure of a call or of the driver, each with its `protocol` code.

use clauseo_protocol::ErrorCode;

/// Why a call or the [`Driver`](crate::Driver) failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The connection is closed: the app-server's stdout ended, writing to its stdin failed, or
    /// the driver ended or was dropped.
    #[error("the Codex app-server connection is closed")]
    Closed,
    /// The app-server answered the call with a JSON-RPC error.
    #[error("the Codex app-server answered with error {code}: {message}")]
    Rpc {
        /// The JSON-RPC code.
        code: i64,
        /// Its message.
        message: String,
    },
    /// The app-server broke the protocol: a line that is not a JSON-RPC message or is over
    /// 8 MiB (which ends the connection), or an answer that does not decode as the call's
    /// response type (which fails only that call).
    #[error("the Codex app-server broke the protocol: {0}")]
    Protocol(String),
}

impl Error {
    /// The code a client sees: `agent.exited` for [`Error::Closed`], else
    /// `agent.protocol_error`.
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Closed => ErrorCode::AgentExited,
            Self::Rpc { .. } | Self::Protocol(_) => ErrorCode::AgentProtocolError,
        }
    }
}
