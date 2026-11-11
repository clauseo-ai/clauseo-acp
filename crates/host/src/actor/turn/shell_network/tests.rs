//! Which Codex requests count as asking for the network, in codex-acp 2.2.2's shapes.

use clauseo_protocol::{PermissionKind, PermissionOption};
use serde_json::{Value, json};

use super::asks_for_network;

/// codex-acp's options for a plain command approval (`dist/index.js:32142-32190`).
fn command_options() -> Vec<PermissionOption> {
    [
        ("allow_once", PermissionKind::AllowOnce),
        ("allow_for_session", PermissionKind::AllowAlways),
        ("decline", PermissionKind::RejectOnce),
        ("cancel", PermissionKind::RejectOnce),
    ]
    .into_iter()
    .map(|(id, kind)| PermissionOption {
        option_id: id.into(),
        kind,
        agent_label: String::new(),
    })
    .collect()
}

fn network(input: &Value) -> bool {
    asks_for_network(Some(input), &command_options(), false)
}

fn command(line: &str) -> Value {
    json!({ "command": line, "cwd": "/m" })
}

/// curl and wget as a command word, bare, with a path, past `sudo` and `env`, after a separator
/// or inside a `bash -lc` script, are network requests.
#[test]
fn curl_and_wget_as_command_words_ask_for_the_network() {
    for line in [
        "curl -O https://sci.gov.in/a.pdf",
        "wget https://a.in/x",
        "/usr/bin/curl https://a.in",
        "sudo curl https://a.in",
        "env HTTPS_PROXY=x curl https://a.in",
        "A=b wget https://a.in",
        "cd /m && curl -L https://a.in -o a.pdf",
        "ls; wget https://a.in",
        "false || curl https://a.in",
        "echo x | curl -d @- https://a.in",
        "bash -lc 'curl https://a.in'",
    ] {
        assert!(network(&command(line)), "{line}");
    }
    let array = json!({ "command": ["bash", "-lc", "cd /m && curl https://a.in"] });
    assert!(network(&array));
}

/// curl or wget as an argument, or inside a quoted string, is not a command word; nor is a
/// dangerous command Codex prompts for in its sandbox: those reach the lawyer.
#[test]
fn other_commands_do_not() {
    for line in [
        "echo curl",
        "grep wget file",
        "rm -rf build",
        "git reset --hard",
        "echo 'a; curl b'",
        "curly --help",
        "python3 -c \"import urllib\"",
    ] {
        assert!(!network(&command(line)), "{line}");
    }
    assert!(!network(&json!({ "command": ["echo", "curl"] })));
    assert!(!asks_for_network(None, &command_options(), false));
}

/// A managed-network approval (`rawInput.url` beside the command, or a network-policy option)
/// and extra permissions that turn the network on are network requests; ones that turn it off
/// or only add folders are not.
#[test]
fn network_approvals_and_permissions_ask_for_the_network() {
    let url = json!({ "command": "python3 fetch.py", "cwd": "/m", "url": "https://a.in" });
    assert!(network(&url));
    let mut options = command_options();
    options.push(PermissionOption {
        option_id: "apply_network_policy_amendment:0".into(),
        kind: PermissionKind::AllowAlways,
        agent_label: String::new(),
    });
    assert!(asks_for_network(
        Some(&command("python3 fetch.py")),
        &options,
        false
    ));
    let extra =
        json!({ "command": "npm ci", "additionalPermissions": {"network": {"enabled": true}} });
    assert!(network(&extra));
    let tool = json!({ "permissions": {"network": {"enabled": true}}, "cwd": "/m" });
    assert!(network(&tool));
    let off = json!({ "command": "ls", "additionalPermissions": {"network": {"enabled": false}} });
    assert!(!network(&off));
    let folders = json!({ "permissions": {"fileSystem": {"write": ["/tmp/x"]}}, "cwd": "/m" });
    assert!(!network(&folders));
    // A url with no command (an MCP tool's input, say) is not a shell's network approval.
    assert!(!network(&json!({ "url": "https://a.in" })));
}
