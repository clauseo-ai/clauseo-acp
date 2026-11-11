//! The `mcp.servers` entry types: [`McpServerLaunch`] and what it holds.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One MCP server: a local program (`command`) or a remote URL (`url`), and whether it is on.
/// `host` passes the enabled local ones to the agent in `session/new`. An entry with
/// `command` is local (a `url` or `headers` beside it is ignored, with a warning); one with
/// `url` and no `command` is remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct McpServerLaunch {
    /// How it is reached.
    #[serde(flatten)]
    pub transport: McpTransport,
    /// Whether the agent gets it. Off unless set.
    #[serde(default)]
    pub enabled: bool,
}

/// How an MCP server is reached. On disk the fields sit in the server's entry, with no tag:
/// `command` makes it local, `url` remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum McpTransport {
    /// A program on this computer, speaking MCP on stdio.
    Local {
        /// The program as the lawyer typed it: a bare name (`npx`), which the host finds on this
        /// computer's search paths at each launch, or an absolute path.
        command: String,
        /// Its arguments.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        /// Variables added to its environment, by name. A value is `{"keychain": true}`: it
        /// is in the OS keychain (service `ai.clauseo.desktop.mcp`, account
        /// `<server>/env/<name>`), where Settings › Tools puts every value the lawyer types.
        /// A plain string also works, for a firm's policy or a hand-edited file, but it is
        /// stored as it is in that file and sent to the agent in `session/new`: keep tokens in
        /// the keychain.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        env: BTreeMap<String, McpValue>,
    },
    /// A server at an `https` URL (`http` only for `localhost` or `127.0.0.1`). Stored and
    /// shown; this version sends it to no agent.
    Remote {
        /// The URL.
        url: String,
        /// Headers sent with each request, by name: `{"keychain": true}` (account
        /// `<server>/header/<name>`) or a plain string, as for `env`.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        headers: BTreeMap<String, McpValue>,
    },
}

/// An MCP server's variable or header value: in the keychain, or plain text in the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum McpValue {
    /// `{"keychain": true}`: the value is the keychain item of this server and name.
    Keychain(KeychainRef),
    /// The value itself, in plain text in the file it came from.
    Plain(String),
}

/// A reference to a keychain item: `{"keychain": true}`. The item's account comes from the
/// server's name and the variable's or header's, so the reference names neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeychainRef {
    /// Always `true`.
    pub keychain: Saved,
}

/// `true`, the one value a [`KeychainRef`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Saved;

impl Serialize for Saved {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(true)
    }
}

impl<'de> Deserialize<'de> for Saved {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if bool::deserialize(deserializer)? {
            Ok(Saved)
        } else {
            Err(serde::de::Error::custom("keychain must be true"))
        }
    }
}

impl JsonSchema for Saved {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Saved".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "const": true })
    }
}

impl McpServerLaunch {
    /// A local server.
    pub fn local(
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, McpValue>,
        enabled: bool,
    ) -> Self {
        Self {
            transport: McpTransport::Local { command, args, env },
            enabled,
        }
    }

    /// A local server's program, or `None` for a remote one.
    pub fn command(&self) -> Option<&str> {
        match &self.transport {
            McpTransport::Local { command, .. } => Some(command),
            McpTransport::Remote { .. } => None,
        }
    }

    /// Its variables (a local server's) or headers (a remote one's), by name.
    pub fn values(&self) -> &BTreeMap<String, McpValue> {
        match &self.transport {
            McpTransport::Local { env, .. } => env,
            McpTransport::Remote { headers, .. } => headers,
        }
    }
}
