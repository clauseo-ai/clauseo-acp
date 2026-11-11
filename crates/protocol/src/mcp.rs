//! The lawyer's own MCP servers as Settings › Tools shows them: [`McpToolsView`] rides
//! `HostEvent::McpToolsChanged` (in every app catch-up but for [`McpToolsView::EMPTY`], what a
//! window starts with), and `HostCommand::Mcp`
//! (`command/mcp.rs`) changes the list.
//!
//! No secret value is ever in a view: an env variable or a header names only whether a value is
//! held ([`McpSecretName::saved`]). The values live in the OS keychain, or, for a value a firm
//! policy or a hand-edited `settings.json` holds in plain text, in that file; the host reads
//! them at a session's launch and nowhere else.

use serde::{Deserialize, Serialize};

/// Who decides the list of MCP servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum McpControl {
    /// The lawyer: the list is theirs to change.
    Lawyer,
    /// The firm's policy sets `mcp.servers` (a security key, or a policy source that cannot be
    /// read): the list is the firm's, shown read-only, and every `HostCommand::Mcp` is refused
    /// with `settings.locked`.
    FirmList,
    /// The firm runs MCP itself through Claude Code's enterprise `managed-mcp.json` on this
    /// computer: no server from the list reaches a session, `servers` is empty, and every
    /// `HostCommand::Mcp` is refused with `settings.locked`.
    FirmManaged,
}

impl McpToolsView {
    /// The lawyer's list with no server: what a window holds before any `McpToolsChanged`. An
    /// app catch-up leaves the event out when the view is this.
    pub const EMPTY: Self = Self {
        control: McpControl::Lawyer,
        servers: Vec::new(),
    };
}

impl McpControl {
    /// Every value, in declaration order.
    pub const ALL: [Self; 3] = [Self::Lawyer, Self::FirmList, Self::FirmManaged];
}

/// How a server is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum McpServerKind {
    /// A program on this computer, started with its arguments, speaking MCP on stdio.
    Local,
    /// A server at an `https` URL (or `http` on this computer only), with headers. Stored and
    /// shown; not sent to a session in this version.
    Remote,
}

impl McpServerKind {
    /// Every value, in declaration order.
    pub const ALL: [Self; 2] = [Self::Local, Self::Remote];
}

/// The MCP servers Settings › Tools shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct McpToolsView {
    /// Whose list it is.
    pub control: McpControl,
    /// The servers, in name order; empty under [`McpControl::FirmManaged`].
    pub servers: Vec<McpServerView>,
}

/// One server, without any secret value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct McpServerView {
    /// Its name: 1 to 64 letters, digits, `_` or `-`.
    pub name: String,
    /// How it is reached.
    pub kind: McpServerKind,
    /// A local server's program as the lawyer typed it: a bare name (`npx`) or an absolute path;
    /// left out for a remote one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// A local server's arguments; empty for a remote one.
    pub args: Vec<String>,
    /// A remote server's URL; left out for a local one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// A local server's variables, by name in name order; never a value. Empty for a remote one.
    pub env: Vec<McpSecretName>,
    /// A remote server's headers, by name in name order; never a value. Empty for a local one.
    pub headers: Vec<McpSecretName>,
    /// Whether sessions get it (both plans alike).
    pub enabled: bool,
}

/// A variable's or a header's name, and whether a value is held for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct McpSecretName {
    /// The name, such as `GITHUB_TOKEN` or `Authorization`.
    pub name: String,
    /// Whether a value is held: in the keychain, or in plain text in a firm policy or a
    /// hand-edited `settings.json`. `false` when the settings name a keychain item the host
    /// found missing or unreadable at a launch since it started; the server is then left out of
    /// sessions until the value is set again.
    pub saved: bool,
}
