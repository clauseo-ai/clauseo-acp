//! The lawyer's own MCP servers as Settings › Tools shows them: [`McpToolsView`] rides
//! `HostEvent::McpToolsChanged` (in every app catch-up), and `HostCommand::Mcp`
//! (`command/mcp.rs`) changes the list.
//!
//! No secret value is ever in a view: an env variable or a header names only whether a value is
//! held ([`McpSecretName::saved`]). The values live in the OS keychain, or, for a value a firm
//! policy or a hand-edited `settings.json` holds in plain text, in that file; the host reads
//! them at a session's launch and nowhere else.
//!
//! The view also names what Claude Desktop, Claude Code and Codex set up that the lawyer can
//! import ([`McpImportable`]), and how the last import went ([`McpImportResult`]).

use serde::{Deserialize, Serialize};

use crate::CommandId;

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
    /// The lawyer's list with no server.
    pub const EMPTY: Self = Self {
        control: McpControl::Lawyer,
        servers: Vec::new(),
        importable: Vec::new(),
        last_import: None,
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
    /// Each app whose servers can be imported now, in [`McpImportSource::ALL`]'s order, with
    /// how many would import: a server that would be skipped (any [`McpSkipReason`]) does not
    /// count, and an app with none is left out. Empty while the firm decides the list. The host
    /// reads the apps' config at its start and at each app catch-up (a window opening), off its
    /// own thread, so the counts can be a little behind the files.
    pub importable: Vec<McpImportable>,
    /// How the last `McpCommand::Import` since the host started went; left out before any.
    /// Its `command` is the import's command id: a window matches it to its own command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_import: Option<McpImportResult>,
}

/// An app whose MCP servers the lawyer can import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum McpImportSource {
    /// Claude Desktop's config file.
    ClaudeDesktop,
    /// Claude Code: its user-wide servers and each project's, as one source.
    ClaudeCode,
    /// Codex's `config.toml` (the screen names it `ChatGPT`).
    Codex,
}

impl McpImportSource {
    /// Every value, in declaration order.
    pub const ALL: [Self; 3] = [Self::ClaudeDesktop, Self::ClaudeCode, Self::Codex];
}

/// One app with servers to import, and how many would import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct McpImportable {
    /// The app.
    pub source: McpImportSource,
    /// How many of its servers an import would add; never 0.
    pub count: u32,
}

/// What one `McpCommand::Import` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct McpImportResult {
    /// The import's command id.
    pub command: CommandId,
    /// The app imported from.
    pub source: McpImportSource,
    /// The servers added, by their name in Clauseo, in name order.
    pub imported: Vec<String>,
    /// The app's servers left out, in name order, each with why.
    pub skipped: Vec<McpSkipped>,
}

/// A server an import left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct McpSkipped {
    /// Its name in Clauseo.
    pub name: String,
    /// Why it was left out.
    pub reason: McpSkipReason,
    /// The program as the app's config names it, for `command_not_found`; left out otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
}

/// Why an import left a server out. A server with more than one of these gets the first in
/// declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum McpSkipReason {
    /// Clauseo already has a server of this name.
    AlreadyAdded,
    /// A server in a project's `.mcp.json` that Claude Code has not approved.
    NotApproved,
    /// The app reaches it in a way Clauseo does not (such as `sse`), or at a URL Clauseo does
    /// not allow (`http` to another computer).
    UnsupportedTransport,
    /// Its program is not on this computer (`program` names it).
    CommandNotFound,
    /// Its config names a variable that is not set.
    UnresolvedVariable,
    /// Part of its entry could not be read, or Clauseo's rules refuse it.
    Unreadable,
}

impl McpSkipReason {
    /// Every value, in declaration order.
    pub const ALL: [Self; 6] = [
        Self::AlreadyAdded,
        Self::NotApproved,
        Self::UnsupportedTransport,
        Self::CommandNotFound,
        Self::UnresolvedVariable,
        Self::Unreadable,
    ];
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
