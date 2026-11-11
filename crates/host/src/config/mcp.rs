//! The keychain seam of the lawyer's MCP servers (Settings › Tools), kept apart from
//! `config.rs`.

use std::sync::Arc;

use clauseo_connectors::SecretStore;

use super::HostConfig;

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
