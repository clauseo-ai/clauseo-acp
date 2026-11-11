//! The keychain the lawyer's MCP server values live in (Settings › Tools): the connectors'
//! keychain (`connectors::secret_store`), under its own service,
//! `clauseo_settings::MCP_KEYCHAIN_SERVICE`. Kept out of `connectors.rs`, which reads no
//! settings.

use std::sync::Arc;

use clauseo_connectors::KeyringStore;
use clauseo_host::HostConfig;

/// `config` with that keychain; unchanged with none (`None`, as on Linux), so no value can be
/// saved and no server naming one starts.
pub(crate) fn with_mcp_secrets(config: HostConfig, store: Option<KeyringStore>) -> HostConfig {
    match store {
        Some(store) => config.with_mcp_secrets(Arc::new(
            store.with_service(clauseo_settings::MCP_KEYCHAIN_SERVICE),
        )),
        None => config,
    }
}
