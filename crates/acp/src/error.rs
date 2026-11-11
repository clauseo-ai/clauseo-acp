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
    /// The agent answered with a JSON-RPC error, or its answer did not decode (the SDK's
    /// `-32700`). The driver has logged it (`diagnose.rs`).
    #[error("the agent answered {method} with error {code}: {message}")]
    Rpc {
        /// The request's JSON-RPC method, such as `session/new`.
        method: String,
        /// The JSON-RPC code.
        code: i32,
        /// Its message: the agent's text, never for the log file.
        message: String,
        /// Its `data`'s shape, fit for the log file: `none`, a scalar's kind, or an object's
        /// key names (never a value).
        data: String,
        /// Its `data` as text: the string at `data.message` when there is one (codex-acp puts
        /// Codex's own error there, under a bare "Internal error"), else the JSON, at most
        /// 4 KiB ([`data_text`]). For classifying only ([`Error::rpc_data`]): never in the
        /// error's text nor the log, as it may quote the lawyer's words.
        data_text: Option<String>,
        /// The few named fields of its `data` that classify it, and nothing else
        /// ([`RpcData`]): never the lawyer's words.
        data_fields: RpcData,
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

/// The allowlisted fields of an RPC error's `data`, each kept only when it is a short token
/// (at most [`MAX_FIELD_BYTES`] of ASCII letters, digits, `_`, `-` and `.`), so no text the
/// lawyer wrote can ride along: an adapter's error data may quote the prompt. Read by
/// `map/too_long.rs` to tell a too-long refusal from other errors.
///
/// Boxed, and `None` when no field is kept, so `Error` stays small.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RpcData(Option<Box<RpcFields>>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct RpcFields {
    error_kind: Option<String>,
    terminal_reason: Option<String>,
    codex_error_info: Option<String>,
}

/// The most bytes of one [`RpcData`] field.
pub(crate) const MAX_FIELD_BYTES: usize = 64;

impl RpcData {
    /// The allowlisted fields of `data`: `errorKind` (claude-agent-acp), `terminal_reason` (the
    /// Claude SDK's) and `codex_error_info` (codex-acp's, or `codexErrorInfo` as Codex's
    /// app-server spells it, when it is a plain string). Any other field, and any value that is
    /// not a short token, is dropped.
    pub(crate) fn read(data: Option<&serde_json::Value>) -> Self {
        let field = |key: &str| {
            data.and_then(|data| data.get(key))
                .and_then(serde_json::Value::as_str)
                .filter(|value| is_token(value))
                .map(str::to_owned)
        };
        let fields = RpcFields {
            error_kind: field("errorKind"),
            terminal_reason: field("terminal_reason"),
            codex_error_info: field("codex_error_info").or_else(|| field("codexErrorInfo")),
        };
        let kept = fields.error_kind.is_some()
            || fields.terminal_reason.is_some()
            || fields.codex_error_info.is_some();
        Self(kept.then(|| Box::new(fields)))
    }

    fn fields(&self) -> Option<&RpcFields> {
        self.0.as_deref()
    }

    /// `data.errorKind`.
    pub fn error_kind(&self) -> Option<&str> {
        self.fields()?.error_kind.as_deref()
    }

    /// `data.terminal_reason`.
    pub fn terminal_reason(&self) -> Option<&str> {
        self.fields()?.terminal_reason.as_deref()
    }

    /// `data.codex_error_info` (or `codexErrorInfo`), when a string.
    pub fn codex_error_info(&self) -> Option<&str> {
        self.fields()?.codex_error_info.as_deref()
    }
}

fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_FIELD_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

/// The most of an RPC error's `data` kept.
pub(crate) const MAX_DATA_BYTES: usize = 4096;

/// The text of an RPC error's `data`: its `message` string when it has one, else its JSON, cut
/// at a char boundary within [`MAX_DATA_BYTES`]. `None` for no data or a JSON `null`.
pub(crate) fn data_text(data: Option<&serde_json::Value>) -> Option<String> {
    let data = data.filter(|data| !data.is_null())?;
    let mut text = match data.get("message").and_then(serde_json::Value::as_str) {
        Some(message) => message.to_owned(),
        None => match data.as_str() {
            Some(text) => text.to_owned(),
            None => data.to_string(),
        },
    };
    if text.len() > MAX_DATA_BYTES {
        let mut end = MAX_DATA_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Some(text)
}

impl Error {
    /// An RPC error's `data` as text, for classifying the failure (offline, sign in again);
    /// `None` for any other error.
    pub fn rpc_data(&self) -> Option<&str> {
        let Self::Rpc { data_text, .. } = self else {
            return None;
        };
        data_text.as_deref()
    }

    /// Whether the agent refused the prompt as too long to read (`map/too_long.rs`): an RPC
    /// error whose allowlisted `data` fields, or Codex's exact refusal text, say so. The host
    /// ends such a turn `TooLong`, never `Failed`.
    pub fn prompt_too_long(&self) -> bool {
        let Self::Rpc {
            message,
            data_text,
            data_fields,
            ..
        } = self
        else {
            return false;
        };
        crate::map::rpc_says_too_long(message, data_fields, data_text.as_deref())
    }

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
