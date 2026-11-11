//! The keychain seam of the lawyer's MCP servers (Settings › Tools), kept apart from
//! `config.rs`.

use std::sync::Arc;

use clauseo_connectors::SecretStore;

use super::HostConfig;

/// What the confirm before a lawyer's MCP server is added, or its launch changed, shows
/// (`HostConfig::with_mcp_confirm`): the program and its arguments as the lawyer typed them, or
/// a remote server's URL. Never a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpLaunchAsk {
    /// The server's name.
    pub server: String,
    /// What starts, verbatim: a local server's command as typed, then each argument; empty for a
    /// remote server.
    pub program: Vec<String>,
    /// A remote server's URL; `None` for a local one.
    pub url: Option<String>,
}

/// The native confirm before a lawyer's MCP server is added or its launch changes (its command,
/// arguments, variable names or URL): `true` when the lawyer allows it. It blocks: the host
/// calls it off the actor, on its keychain thread, one at a time, after the program is found
/// and before anything is written. Desktop draws it in Rust, as the pay link's.
pub trait McpLaunchConfirm: Send + Sync {
    /// Asks about `ask`; `true` allows the save.
    fn confirm(&self, ask: &McpLaunchAsk) -> bool;
}

/// Finds a lawyer's MCP server's program: the command as typed (a bare name or an absolute path)
/// to the program's path, or `None` when there is none (`HostConfig::with_mcp_resolver`).
pub type McpResolver = Arc<dyn Fn(&str) -> Option<std::path::PathBuf> + Send + Sync>;

#[expect(
    clippy::multiple_inherent_impl,
    reason = "config.rs is at its 600-line limit; the MCP servers' seams live in their own file"
)]
impl HostConfig {
    /// The keychain the lawyer's MCP server values are written to and read from: desktop's
    /// `KeyringStore` under `clauseo_settings::MCP_KEYCHAIN_SERVICE`, each value under
    /// `clauseo_settings::keychain_account`. Unset (the default, and on Linux), a save that
    /// sets a value is refused with `mcp.secret_store_failed`, and a server whose settings name
    /// a keychain value is left out of every session.
    #[must_use]
    pub fn with_mcp_secrets(mut self, secrets: Arc<dyn SecretStore>) -> Self {
        self.wiring.mcp_secrets = Some(secrets);
        self
    }

    /// The confirm a `SaveServer` that adds a server, or changes its command, arguments,
    /// variable names or URL, must pass (`McpLaunchConfirm`): declined, the save answers
    /// `mcp.save_declined` with nothing written. A rename, a turn on or off, or new values alone
    /// ask nothing. Unset, every such save is declined: no settings-window script adds a program
    /// unseen.
    #[must_use]
    pub fn with_mcp_confirm(mut self, confirm: Arc<dyn McpLaunchConfirm>) -> Self {
        self.wiring.mcp_confirm = Some(confirm);
        self
    }

    /// How a lawyer's MCP server's program is found, at a save (`mcp.command_not_found` when it
    /// is not) and at each launch (the server left out when it is not): `resolve` takes the
    /// command as typed, a bare name or an absolute path, and answers the program's path. It runs
    /// off the actor. Unset, `clauseo_settings::mcp_import::resolve_command` over this process's
    /// environment (tests set their own).
    #[must_use]
    pub fn with_mcp_resolver(mut self, resolve: McpResolver) -> Self {
        self.wiring.mcp_resolve = resolve;
        self
    }
}
