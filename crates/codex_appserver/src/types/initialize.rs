//! `initialize`: `InitializeParams` and `InitializeResponse` (schema `v1/`).

use serde::{Deserialize, Serialize};

/// `initialize`'s params. The app-server takes no protocol version (research 27 §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// Who is connecting.
    pub client_info: ClientInfo,
    /// What the client opts into; [`Client::initialize`](crate::Client::initialize) sends an
    /// empty object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<InitializeCapabilities>,
}

/// `ClientInfo`: the connecting client's name and version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    /// The client's name.
    pub name: String,
    /// A display title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The client's version.
    pub version: String,
}

impl ClientInfo {
    /// Clauseo's: `{ "name": "clauseo", "version": <this crate's version> }`.
    pub fn clauseo() -> Self {
        Self {
            name: "clauseo".to_owned(),
            title: None,
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

/// `InitializeCapabilities`: what the client opts into. Every field is optional, and
/// [`Client`](crate::Client) sets none of them; above all never `experimentalApi`, since the
/// calls it makes are stable at the pinned version.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeCapabilities {
    /// Opts into experimental methods and fields (the server's default is `false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental_api: Option<bool>,
    /// Uses explicit gateway OAuth sign-in instead of automatic browser authorization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_gateway_oauth: Option<bool>,
    /// MCP extension settings, passed through as given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<serde_json::Map<String, serde_json::Value>>,
    /// The legacy opt-in for the `openai/form` MCP extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server_openai_form_elicitation: Option<bool>,
    /// Notification methods the server should not send on this connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opt_out_notification_methods: Option<Vec<String>>,
    /// Opts into `attestation/generate` requests (the server's default is `false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_attestation: Option<bool>,
}

/// `initialize`'s response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    /// The absolute path of the server's `$CODEX_HOME`.
    pub codex_home: String,
    /// The server's platform family, such as `"unix"` or `"windows"`.
    pub platform_family: String,
    /// The server's operating system, such as `"macos"`, `"linux"` or `"windows"`.
    pub platform_os: String,
    /// The server's user agent.
    pub user_agent: String,
}
