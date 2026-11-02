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
    /// group), MCP servers (they go in `session/new`), R15's override tables, and the host's
    /// `web_search`, which follows `tools.web_search`.
    pub const RESERVED: [&'static str; 6] = [
        "hooks",
        "mcp_servers",
        "features",
        "cloud",
        "skills",
        "web_search",
    ];

    /// `keys` less every [`Self::RESERVED`] key, each dropped with a warning.
    pub fn new(mut keys: Map<String, Value>) -> Self {
        keys.retain(|key, _| {
            let reserved = Self::RESERVED.contains(&key.as_str());
            if reserved {
                tracing::warn!(
                    key,
                    "a reserved CODEX_CONFIG key from the profile is dropped"
                );
            }
            !reserved
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
