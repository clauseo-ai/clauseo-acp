//! The MCP start-up wait in a session's `_meta` ([`mcp_wait_meta`]): set for Codex only, over
//! whatever the profile hook gave, beside the switches [`session_meta`] forces.

use clauseo_protocol::{AgentId, AgentKind};
use serde_json::{Map, Value, json};

use super::{MCP_WAIT_KEY, MCP_WAIT_MS, mcp_wait_meta};
use crate::agent::loads::{McpControl, session_meta};

fn object(value: Value) -> Map<String, Value> {
    let Value::Object(map) = value else {
        panic!("not an object: {value}");
    };
    map
}

fn kind(id: &str) -> Option<AgentKind> {
    let id: AgentId = id.parse().unwrap();
    AgentKind::of(&id)
}

#[test]
fn a_codex_session_waits_ten_seconds_for_its_mcp_servers() {
    assert_eq!(MCP_WAIT_KEY, "mcpStartupAwaitTimeoutMs");
    assert_eq!(MCP_WAIT_MS, 10_000);
    for control in [McpControl::Clauseo, McpControl::Managed] {
        let mut meta = session_meta(None, control, None, None);
        mcp_wait_meta(&mut meta, kind("codex-acp"));
        assert_eq!(
            meta["mcpStartupAwaitTimeoutMs"],
            json!(10000),
            "{control:?}"
        );
        assert!(meta["claudeCode"]["options"].is_object(), "{control:?}");
    }
}

#[test]
fn a_claude_session_and_an_agent_with_no_kind_get_no_wait() {
    for id in ["claude-acp", "fake-agent"] {
        let mut meta = session_meta(None, McpControl::Clauseo, None, None);
        let before = meta.clone();
        mcp_wait_meta(&mut meta, kind(id));
        assert!(!meta.contains_key("mcpStartupAwaitTimeoutMs"), "{id}");
        assert_eq!(meta, before, "{id}");
    }
}

/// A hook value of any type (a shorter wait, none, text, an object) is replaced, and every
/// other hook key stays.
#[test]
fn a_hook_value_cannot_remove_or_change_the_codex_wait() {
    for given in [
        json!(0),
        json!(-1),
        json!(60000),
        json!(null),
        json!("10"),
        json!({}),
    ] {
        let hook = object(json!({"mcpStartupAwaitTimeoutMs": given, "clauseo_test": true}));
        let mut meta = session_meta(Some(hook), McpControl::Clauseo, None, None);
        mcp_wait_meta(&mut meta, kind("codex-acp"));
        assert_eq!(meta["mcpStartupAwaitTimeoutMs"], json!(10000), "{given}");
        assert_eq!(meta["clauseo_test"], json!(true), "{given}");
    }
}
