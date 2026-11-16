//! The lawyer's MCP server commands by window (Settings › Tools): each server is a program that
//! runs with the lawyer's access, so only the settings window adds, changes, turns on or removes
//! one. `main` shows agent output and sends none; another label sends none. Each refusal is
//! `command.invalid` from the bridge, before the host sees the command, and quotes no value.

use serde_json::{Value, json};

use super::*;

/// Every MCP server command, as the settings window writes it.
fn commands() -> Vec<Value> {
    vec![
        json!({"save_server": {"server": {"name": "github", "kind": "local", "command": "npx",
            "env": [{"name": "TOKEN", "value": {"set": "typed-value-for-the-window-test"}}],
            "enabled": true}}}),
        json!({"save_server": {"previous_name": "github", "server": {"name": "gh",
            "kind": "local", "command": "npx",
            "env": [{"name": "TOKEN", "value": {"keep": {}}}], "enabled": false}}}),
        json!({"remove_server": {"name": "github"}}),
        json!({"set_server_enabled": {"name": "github", "enabled": true}}),
    ]
}

/// The table, through `admit`: the settings window every command, `main` and another label none.
#[test]
fn each_mcp_command_is_admitted_from_the_settings_window_only() {
    let scratch = Scratch::new("admit-mcp", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    for body in commands() {
        let text = body.to_string();
        let command: HostCommand = serde_json::from_value(json!({ "mcp": body }))
            .unwrap_or_else(|err| panic!("{text}: {err}"));
        assert!(bridge.admit(SETTINGS, &command).is_ok(), "settings: {text}");
        for label in [MAIN, "other"] {
            let refused = bridge.admit(label, &command).unwrap_err();
            assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{label}: {text}");
            assert!(!refused.message().contains("typed-value"), "{refused}");
        }
    }
}
