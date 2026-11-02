//! Codex's part: the keys the profile adds to `CODEX_CONFIG`.

use serde_json::{Map, Value};

/// The keys the profile adds to a Codex session's `CODEX_CONFIG`. The host merges them after
/// R15's overrides and before `hooks`, adding a key only where none is (a profile key never
/// replaces an override), and writes the object with `serde_json`, so the instructions'
/// quotes and newlines are escaped.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CodexConfig(Map<String, Value>);

impl CodexConfig {
    /// Top-level keys the profile never sends: the hooks seam (the guard's one `PreToolUse`
    /// group), MCP servers (they go in `session/new`), R15's override tables, the host's
    /// `web_search`, which follows `tools.web_search`, and `bypass_hook_trust`, which
    /// codex-acp reads straight from the request (codex `rust-v0.162.1`,
    /// `codex-rs/app-server/src/config_manager.rs:438`).
    pub const RESERVED: [&'static str; 7] = [
        "hooks",
        "mcp_servers",
        "features",
        "cloud",
        "skills",
        "web_search",
        "bypass_hook_trust",
    ];

    /// `keys` less every key that could reach a reserved table, each dropped with a warning: a
    /// key holding `.`, and a key whose first `.`-separated part is one of [`Self::RESERVED`].
    ///
    /// codex reads each request key as a dotted path (at `rust-v0.162.1`,
    /// `codex-rs/app-server/src/config_manager.rs:446-454` chains the request's keys into the
    /// CLI overrides, and `codex-rs/config/src/overrides.rs:18-22` splits each on `.`), so
    /// `"hooks.PreToolUse"` would write into `hooks`. The host's merge must treat keys the
    /// same way: a dotted key from anywhere is a clash with its first part.
    pub fn new(mut keys: Map<String, Value>) -> Self {
        keys.retain(|key, _| {
            let first = key.split('.').next().unwrap_or_default();
            let refused = key.contains('.') || Self::RESERVED.contains(&first);
            if refused {
                tracing::warn!(
                    key,
                    "a reserved or dotted CODEX_CONFIG key from the profile is dropped"
                );
            }
            !refused
        });
        Self(keys)
    }

    /// `{"developer_instructions": instructions}`.
    pub fn with_instructions(instructions: String) -> Self {
        let mut keys = Map::new();
        keys.insert(
            "developer_instructions".to_owned(),
            Value::String(instructions),
        );
        Self::new(keys)
    }

    /// The keys.
    pub fn keys(&self) -> &Map<String, Value> {
        &self.0
    }

    /// The keys, owned.
    pub fn into_keys(self) -> Map<String, Value> {
        self.0
    }
}
