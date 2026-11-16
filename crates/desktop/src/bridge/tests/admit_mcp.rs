//! The lawyer's MCP server commands (Settings › Tools) are the settings window's alone: an
//! import, like a save, adds programs that run as the lawyer, so the window that shows agent
//! output may not send it. Split from `admit.rs` for its length.

use serde_json::json;

use super::*;

/// A command as the page writes it.
fn command(body: serde_json::Value) -> HostCommand {
    let text = body.to_string();
    serde_json::from_value(body).unwrap_or_else(|err| panic!("{text}: {err}"))
}

/// `import` for each app, and `remove_server`, pass from the settings window and are refused
/// from `main` and any other label with `command.invalid`, before the host sees them.
#[test]
fn the_mcp_import_comes_from_the_settings_window_only() {
    let scratch = Scratch::new("admit-mcp", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let mut commands: Vec<HostCommand> = ["claude_desktop", "claude_code", "codex"]
        .into_iter()
        .map(|source| command(json!({"mcp": {"import": {"source": source}}})))
        .collect();
    commands.push(command(
        json!({"mcp": {"remove_server": {"name": "github"}}}),
    ));
    for mcp in &commands {
        assert!(bridge.admit(SETTINGS, mcp).is_ok(), "{mcp:?}");
        for label in [MAIN, "other"] {
            let refused = bridge.admit(label, mcp).unwrap_err();
            assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{label} {mcp:?}");
        }
        let refusal = block_on(bridge.dispatch(MAIN, envelope(mcp.clone()))).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{mcp:?}");
    }
}
