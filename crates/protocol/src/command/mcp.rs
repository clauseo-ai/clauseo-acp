//! The lawyer's MCP server commands, in one `HostCommand::Mcp` variant:
//! `{"mcp": {"remove_server": {"name": "github"}}}`.
//!
//! Each answers `Done` or a refusal, and stores a receipt as the other settings commands do; the
//! new list reaches app subscribers as `HostEvent::McpToolsChanged`. A secret value travels
//! only in [`McpSecretValue::Set`], as a [`RedactedString`]: it encodes and prints as
//! `[redacted]`, so it reaches no receipt hash, log or snapshot, and a decode error quotes none
//! of the input. The host writes it to the OS keychain and nowhere else.
//!
//! The refusals (each an [`ErrorCode`](crate::ErrorCode)): `mcp.name_empty`,
//! `mcp.name_taken`, `mcp.name_reserved`, `mcp.name_invalid`, `mcp.command_invalid`,
//! `mcp.command_not_found`, `mcp.save_declined` (the lawyer's native confirm, before a save that
//! adds a server or changes its program, arguments, variable names or URL, answered Cancel),
//! `mcp.url_invalid`, `mcp.header_name_invalid`, `mcp.env_name_invalid`,
//! `mcp.secret_store_failed`, `mcp.server_unknown`, and `settings.locked` while the firm decides
//! the list (`McpControl::FirmList` or `FirmManaged`). A shape this module refuses (a field of
//! the other kind, a list too long, a keep for a name with nothing to keep) is
//! `command.invalid`.

use serde::{Deserialize, Deserializer, Serialize};

use crate::{Error, McpServerKind, RedactedString};

/// Most bytes in a server name, a variable's or a header's name as a command carries it. The
/// host refuses a name over 64 bytes as `mcp.name_invalid`; this bound only keeps a command
/// small.
pub const MAX_MCP_NAME_BYTES: usize = 256;

/// Most bytes in a local server's command, one of its arguments, or a remote server's URL.
pub const MAX_MCP_TEXT_BYTES: usize = 4096;

/// Most arguments, variables or headers one server holds.
pub const MAX_MCP_LIST: usize = 128;

/// What a client asks of the lawyer's MCP servers. Unknown fields and kinds are refused, as in
/// every command. A decode error quotes none of the input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum McpCommand {
    /// Add a server (`previous_name` null) or replace the one named `previous_name`, which may
    /// be renamed. Secret values are written to the keychain first, then the settings; when
    /// the settings write fails the keychain is put back as far as it can be. A variable or
    /// header the server held before and `server` no longer names is removed, and its keychain
    /// item deleted. A rename moves the server's keychain items to the new name.
    SaveServer {
        /// The server this replaces; left out (or null) to add one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous_name: Option<String>,
        /// The server as it is to be.
        server: McpServerEdit,
    },
    /// Remove a server and delete its keychain items. Removing a server that is not there
    /// does nothing and answers `Done`.
    RemoveServer {
        /// The server.
        name: String,
    },
    /// Turn a server on or off for both plans. A server not there is `mcp.server_unknown`.
    SetServerEnabled {
        /// The server.
        name: String,
        /// Whether sessions get it.
        enabled: bool,
    },
}

/// A server as `SaveServer` sets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct McpServerEdit {
    /// Its name.
    pub name: String,
    /// How it is reached: a local server takes `command`, `args` and `env`; a remote one `url`
    /// and `headers`. A field of the other kind is `command.invalid`.
    pub kind: McpServerKind,
    /// A local server's program as the lawyer typed it: a bare name (`npx`), which the host
    /// finds on this computer's search paths, or an absolute path; left out for a remote one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// A local server's arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// A remote server's URL: `https`, or `http` for `localhost` or `127.0.0.1` only; left
    /// out for a local one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// A local server's variables.
    #[serde(default)]
    pub env: Vec<McpSecretEdit>,
    /// A remote server's headers.
    #[serde(default)]
    pub headers: Vec<McpSecretEdit>,
    /// Whether sessions get it.
    pub enabled: bool,
}

/// One variable or header of a [`McpServerEdit`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct McpSecretEdit {
    /// The variable's name (a letter or `_`, then letters, digits or `_`) or the header's (an
    /// HTTP token). The same name twice in one list (a header's in any case) is refused.
    pub name: String,
    /// Its value.
    pub value: McpSecretValue,
}

/// What a [`McpSecretEdit`] does with the value. JSON: `{"keep": {}}` or `{"set": "…"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum McpSecretValue {
    /// Keep the value held for this name (under `previous_name`'s server); a name with no value
    /// held is `command.invalid`.
    Keep {},
    /// The new value, as typed. It encodes and prints as `[redacted]`.
    Set(RedactedString),
}

/// The one error an MCP command that does not decode gives: it quotes nothing.
const UNREADABLE: &str = "an MCP server command that could not be read";

/// The kind a shape error names.
const KIND: &str = "McpCommand";

/// [`McpCommand`]'s derived twin, which only decodes; its errors never leave this module.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Wire {
    SaveServer {
        #[serde(default)]
        previous_name: Option<String>,
        server: McpServerEdit,
    },
    RemoveServer {
        name: String,
    },
    SetServerEnabled {
        name: String,
        enabled: bool,
    },
}

/// [`McpSecretValue`]'s derived twin.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum ValueWire {
    Keep {},
    Set(RedactedString),
}

// Decoded through `Wire`, with every error replaced by `UNREADABLE`: serde's own messages quote
// the input, so `{"keep": "<the token>"}` would echo a secret back to the window.
impl<'de> Deserialize<'de> for McpCommand {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Wire::deserialize(deserializer)
            .map_err(|_quotes_input| <D::Error as serde::de::Error>::custom(UNREADABLE))?;
        Ok(match wire {
            Wire::SaveServer {
                previous_name,
                server,
            } => Self::SaveServer {
                previous_name,
                server,
            },
            Wire::RemoveServer { name } => Self::RemoveServer { name },
            Wire::SetServerEnabled { name, enabled } => Self::SetServerEnabled { name, enabled },
        })
    }
}

impl<'de> Deserialize<'de> for McpSecretValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ValueWire::deserialize(deserializer)
            .map_err(|_quotes_input| <D::Error as serde::de::Error>::custom(UNREADABLE))?;
        Ok(match wire {
            ValueWire::Keep {} => Self::Keep {},
            ValueWire::Set(value) => Self::Set(value),
        })
    }
}

impl McpCommand {
    /// The shape rules: every name and text within its bound, no field of the other kind, no
    /// list longer than [`MAX_MCP_LIST`], and no value that is the `[redacted]` placeholder
    /// (what an encoded command decodes back to, never a value the lawyer typed). Whether a name
    /// is well formed, free or reserved, a command absolute or a URL allowed is the host's check,
    /// with its own codes.
    pub(super) fn check(&self) -> Result<(), Error> {
        match self {
            Self::RemoveServer { name } | Self::SetServerEnabled { name, enabled: _ } => bounded(
                name,
                MAX_MCP_NAME_BYTES,
                "a server name is at most 256 bytes",
            ),
            Self::SaveServer {
                previous_name,
                server,
            } => {
                if let Some(previous) = previous_name {
                    bounded(
                        previous,
                        MAX_MCP_NAME_BYTES,
                        "a server name is at most 256 bytes",
                    )?;
                }
                server.check()
            }
        }
    }
}

impl McpServerEdit {
    fn check(&self) -> Result<(), Error> {
        bounded(
            &self.name,
            MAX_MCP_NAME_BYTES,
            "a server name is at most 256 bytes",
        )?;
        let other_kind = match self.kind {
            McpServerKind::Local => self.url.is_some() || !self.headers.is_empty(),
            McpServerKind::Remote => {
                self.command.is_some() || !self.args.is_empty() || !self.env.is_empty()
            }
        };
        if other_kind {
            return Err(format(
                "a local server takes command, args and env; a remote one url and headers",
            ));
        }
        for text in self.command.iter().chain(&self.url).chain(&self.args) {
            bounded(
                text,
                MAX_MCP_TEXT_BYTES,
                "a command, argument or URL is at most 4096 bytes",
            )?;
        }
        if self.args.len() > MAX_MCP_LIST
            || self.env.len() > MAX_MCP_LIST
            || self.headers.len() > MAX_MCP_LIST
        {
            return Err(format(
                "a server holds at most 128 arguments, variables or headers",
            ));
        }
        for edit in self.env.iter().chain(&self.headers) {
            bounded(
                &edit.name,
                MAX_MCP_NAME_BYTES,
                "a name is at most 256 bytes",
            )?;
            if let McpSecretValue::Set(value) = &edit.value
                && value.is_placeholder()
            {
                return Err(format("a value is the redacted placeholder, not a value"));
            }
        }
        Ok(())
    }
}

/// `text` within `max` bytes, or the shape error `rule`.
fn bounded(text: &str, max: usize, rule: &'static str) -> Result<(), Error> {
    if text.len() > max {
        return Err(format(rule));
    }
    Ok(())
}

fn format(rule: &'static str) -> Error {
    Error::Format { kind: KIND, rule }
}
