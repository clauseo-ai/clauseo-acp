//! Which tools pass: the allowlist, the live views off it, and the batch tool off it.

use serde_json::{Value, json};

use super::super::{ALLOWED_TOOLS, LIVE_VIEW_TOOLS, UNAVAILABLE, UNKNOWN_TOOL};
use super::{Verdict, call_tool, refused_on, verdict};

/// A refused call's text, after checking it is an `isError` result with the call's id.
fn refusal_of(message: &Value) -> String {
    let Verdict::Answer(text) = verdict(message) else {
        panic!("forwarded: {message}");
    };
    let answer: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(answer["id"], 7);
    assert_eq!(answer["result"]["isError"], true);
    answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Each live-view tool gets the plain refusal and never reaches agent-browser, whatever its
/// arguments; as a notification it is dropped. None is on the allowlist.
#[test]
fn every_live_view_is_unavailable() {
    let tools = [
        "agent_browser_stream_enable",
        "agent_browser_stream_disable",
        "agent_browser_stream_status",
        "agent_browser_dashboard_start",
        "agent_browser_dashboard_stop",
        "agent_browser_inspect",
    ];
    assert_eq!(LIVE_VIEW_TOOLS, tools);
    for tool in tools {
        assert!(!ALLOWED_TOOLS.contains(&tool), "{tool}");
        assert_eq!(refusal_of(&call_tool(tool, &json!({}))), UNAVAILABLE);
        assert_eq!(
            refusal_of(&call_tool(tool, &json!({"port": 9222}))),
            UNAVAILABLE
        );
        let mut bare = call_tool(tool, &json!({}));
        bare["params"].as_object_mut().unwrap().remove("arguments");
        assert_eq!(refusal_of(&bare), UNAVAILABLE);
        bare.as_object_mut().unwrap().remove("id");
        assert_eq!(verdict(&bare), Verdict::Drop, "{tool}");
    }
}

/// The allowlist is agent-browser v0.41.1's `core` profile, all 29 tools, and each passes.
#[test]
fn every_core_tool_passes() {
    assert_eq!(ALLOWED_TOOLS.len(), 29);
    for tool in ALLOWED_TOOLS {
        assert_eq!(refused_on(tool, &json!({})), None, "{tool}");
    }
}

/// A tool off the allowlist (a real non-core one, a made-up or renamed one, a missing or
/// non-string name) is refused with its own text, distinct from a live view's, and never
/// reaches agent-browser; as a notification it is dropped.
#[test]
fn an_unknown_tool_is_refused() {
    assert_ne!(UNKNOWN_TOOL, UNAVAILABLE);
    for tool in [
        "agent_browser_connect",
        "agent_browser_get_cdp_url",
        "agent_browser_install",
        "agent_browser_plugin_run",
        "agent_browser_record_start",
        "agent_browser_stream_enable_v2",
        "agent_browser_Open",
        "",
    ] {
        assert_eq!(
            refusal_of(&call_tool(tool, &json!({}))),
            UNKNOWN_TOOL,
            "{tool}"
        );
    }
    let mut odd = call_tool("x", &json!({}));
    odd["params"]["name"] = json!(["agent_browser_open"]);
    assert_eq!(refusal_of(&odd), UNKNOWN_TOOL);
    odd["params"].as_object_mut().unwrap().remove("name");
    assert_eq!(refusal_of(&odd), UNKNOWN_TOOL);
    odd["params"] = json!("agent_browser_open");
    assert_eq!(refusal_of(&odd), UNKNOWN_TOOL);
    odd.as_object_mut().unwrap().remove("id");
    assert_eq!(verdict(&odd), Verdict::Drop);
}

/// `agent_browser_batch` is not in the core profile, so every batch is refused whole, a string
/// item naming a live view and an unknown command included; nothing inside it reaches
/// agent-browser.
#[test]
fn every_batch_is_refused() {
    for commands in [
        json!({"commands": [["open", "https://example.com"], "stream enable"]}),
        json!({"commands": ["inspect"]}),
        json!({"commands": [["open", "https://example.com"], ["frobnicate"]]}),
        json!({"commands": [["open", "https://example.com"]]}),
    ] {
        assert_eq!(
            refusal_of(&call_tool("agent_browser_batch", &commands)),
            UNKNOWN_TOOL
        );
    }
}

/// The guard runs unfenced, as Codex's MCP server, so a tool that could name a file outside the
/// browser's own folders would reach files the agent's user can read. None of these tools is
/// core, so each is refused whole with [`UNKNOWN_TOOL`]. The core tools that take a path are
/// refused by their argument instead: the `path` key (`REFUSED_KEYS`) and a `file:` URL.
#[test]
fn no_tool_can_name_a_file_outside_the_browser_folder() {
    for (tool, arguments) in [
        (
            "agent_browser_upload",
            json!({"selector": "#f", "files": ["/Users/someone/Library/Application Support/Clauseo/login.json"]}),
        ),
        (
            "agent_browser_upload",
            json!({"selector": "#f", "files": ["/home/someone/.ssh/id_ed25519"]}),
        ),
        (
            "agent_browser_pdf",
            json!({"path": "/home/someone/.ssh/x.pdf"}),
        ),
        (
            "agent_browser_download",
            json!({"selector": "a", "path": "/home/someone/.ssh/x"}),
        ),
        (
            "agent_browser_state_save",
            json!({"path": "/home/someone/.ssh/state.json"}),
        ),
        (
            "agent_browser_state_load",
            json!({"path": "/home/someone/.ssh/state.json"}),
        ),
        (
            "agent_browser_cookies_set",
            json!({"url": "file:///etc/passwd"}),
        ),
    ] {
        assert!(!ALLOWED_TOOLS.contains(&tool), "{tool}");
        assert_eq!(
            refusal_of(&call_tool(tool, &arguments)),
            UNKNOWN_TOOL,
            "{tool}"
        );
    }
    // A core tool's path argument is refused by name, whatever the tool.
    assert_eq!(
        refused_on(
            "agent_browser_screenshot",
            &json!({"path": "/home/someone/.ssh/shot.png"})
        )
        .as_deref(),
        Some("path")
    );
    // A core tool's URL to a local file is refused by its scheme.
    assert_eq!(
        refused_on(
            "agent_browser_open",
            &json!({"url": "file:///home/someone/.ssh/id_ed25519"})
        )
        .as_deref(),
        Some("file URLs")
    );
}
