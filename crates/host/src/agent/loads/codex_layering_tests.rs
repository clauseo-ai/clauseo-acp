//! The layering of what the launch profile and the host give a Codex session: its
//! `CODEX_CONFIG` keys, an allowlist, before R15's pins. Claude's `_meta` is in
//! `layering_tests.rs`.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::layering_tests::{OFF, ON, folders};
use super::profile::codex_keys;

/// Of the hook's keys only `developer_instructions` goes in, filled; every other is refused,
/// reserved, dotted or merely unknown. The host adds its own `web_search` and writable roots.
#[test]
fn codex_keys_take_only_the_allowlist_and_add_the_hosts() {
    let hook = json!({
        "developer_instructions": "Notes in {matter_notes}; work in {work_folder}.",
        "hooks": {"PreToolUse": []},
        "web_search": "live",
        "mcp_servers.x.command": "/x",
        "model_verbosity": "low",
        "sandbox_workspace_write.writable_roots": ["/elsewhere"],
        "sandbox_workspace_write": {"network_access": true, "writable_roots": ["/elsewhere"]},
    });
    let keys = codex_keys(hook.as_object().cloned().unwrap(), OFF, folders()).unwrap();
    assert_eq!(
        Value::Object(keys),
        json!({
            "developer_instructions": "Notes in /data/agent-state/m/notes; work in /data/run/t-1/work.",
            "web_search": "disabled",
            "sandbox_workspace_write": {"writable_roots": ["/data/run/t-1/work", "/data/agent-state/m/notes"]},
        })
    );
    let keys = codex_keys(Map::new(), ON, folders()).unwrap();
    assert_eq!(keys["web_search"], json!("live"));
}

/// A profile that tries `hooks`, `sandbox_mode` or any other refused key leaves the launch env
/// exactly as with none of them, and the home's `config.toml` takes no profile input at all.
#[test]
fn a_profile_trying_hooks_or_sandbox_mode_changes_neither_env_nor_config_toml() {
    let home = std::path::Path::new("/data/agents/codex-acp/home");
    let tries = json!({
        "developer_instructions": "text",
        "hooks": {"PreToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "/x"}]}]},
        "sandbox_mode": "danger-full-access",
        "approval_policy": "never",
        "mcp_servers": {"x": {"command": "/x"}},
        "skills": {"bundled": {"enabled": true}},
        "features": {"apps": true},
        "projects": {"/": {"trust_level": "trusted"}},
        "model_providers": {"x": {}},
        "profiles": {"x": {}},
        "notify": ["/x"],
    });
    let clean = json!({"developer_instructions": "text"});
    let env = |hook: &Value| {
        let keys = codex_keys(hook.as_object().cloned().unwrap(), ON, folders()).unwrap();
        crate::agent::codex::launch_env(home, &BTreeMap::new(), keys, None)
    };
    assert_eq!(env(&tries), env(&clean));
    // Even past `codex_keys`, `config` refuses them.
    let raw = tries.as_object().cloned().unwrap();
    let clean_raw = clean.as_object().cloned().unwrap();
    assert_eq!(
        crate::agent::codex::config(&BTreeMap::new(), raw, None),
        crate::agent::codex::config(&BTreeMap::new(), clean_raw, None)
    );
    let toml = crate::agent::codex::home_config();
    for key in [
        "developer_instructions",
        "sandbox_mode",
        "hooks",
        "writable_roots",
        "notify",
    ] {
        assert!(!toml.contains(key), "{key} in config.toml:\n{toml}");
    }
}

/// R15's pins win over the launch profile's side, which goes in before them; the hooks seam's
/// object stays the one `PreToolUse` group, last. With the host's three keys taken out, the
/// built object is exactly R15's.
#[test]
fn codex_config_keeps_every_r15_key_and_the_hooks_seam() {
    let settings = BTreeMap::from([(
        "CODEX_CONFIG".to_owned(),
        json!({"model": "gpt-5", "features": {"apps": true}}).to_string(),
    )]);
    let r15 = crate::agent::codex::config(&settings, Map::new(), None);
    let mut profile = codex_keys(Map::new(), ON, folders()).unwrap();
    profile.insert(
        "features".to_owned(),
        json!({"apps": true, "new_one": true}),
    );
    profile.insert("model".to_owned(), json!("other"));
    profile.insert("developer_instructions".to_owned(), json!("text"));
    let hooks = json!({"PreToolUse": [{"matcher": "guard"}]});
    let mut built = crate::agent::codex::config(&settings, profile, hooks.as_object().cloned());
    assert_eq!(built["web_search"], json!("live"));
    assert_eq!(built["developer_instructions"], json!("text"));
    assert_eq!(
        built["sandbox_workspace_write"]["writable_roots"],
        json!(["/data/run/t-1/work", "/data/agent-state/m/notes"])
    );
    assert_eq!(
        built["hooks"]["PreToolUse"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(built["hooks"]["PreToolUse"][0]["matcher"], json!("guard"));
    assert_eq!(built.keys().next_back().map(String::as_str), Some("hooks"));
    for key in [
        "web_search",
        "developer_instructions",
        "sandbox_workspace_write",
        "hooks",
    ] {
        built.shift_remove(key);
    }
    assert_eq!(built, r15);
}
