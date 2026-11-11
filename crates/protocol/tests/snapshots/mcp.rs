//! Examples for the MCP server goldens (Settings › Tools), and the checks that a value the lawyer
//! typed never encodes, never prints and never comes back in a decode error. A child of
//! `snapshots.rs`, so its snapshots sit in `snapshots/`.

use super::*;
use clauseo_protocol::{
    CommandEnvelope, McpCommand, McpControl, McpSecretEdit, McpSecretName, McpSecretValue,
    McpServerEdit, McpServerKind, McpServerView, McpToolsView, RedactedString,
};
use zeroize::Zeroizing;

/// A value that must never show in a snapshot, a log line or an error.
const TOKEN: &str = "test-value-7Hs9kQ2mXv4LpR8tYw1Zb";

/// The lawyer's list: one local server with a saved and an unsaved variable, one remote.
pub(super) fn tools_view() -> McpToolsView {
    McpToolsView {
        control: McpControl::Lawyer,
        servers: vec![
            McpServerView {
                name: "github".to_owned(),
                kind: McpServerKind::Local,
                command: Some("/opt/homebrew/bin/npx".to_owned()),
                args: vec![
                    "-y".to_owned(),
                    "@modelcontextprotocol/server-github".to_owned(),
                ],
                url: None,
                env: vec![
                    McpSecretName {
                        name: "GITHUB_PERSONAL_ACCESS_TOKEN".to_owned(),
                        saved: true,
                    },
                    McpSecretName {
                        name: "GITHUB_TOOLSETS".to_owned(),
                        saved: false,
                    },
                ],
                headers: Vec::new(),
                enabled: true,
            },
            McpServerView {
                name: "linear".to_owned(),
                kind: McpServerKind::Remote,
                command: None,
                args: Vec::new(),
                url: Some("https://mcp.linear.app/mcp".to_owned()),
                env: Vec::new(),
                headers: vec![McpSecretName {
                    name: "Authorization".to_owned(),
                    saved: true,
                }],
                enabled: false,
            },
        ],
    }
}

fn secret(text: &str) -> RedactedString {
    RedactedString::new(Zeroizing::new(text.to_owned())).unwrap()
}

/// `SaveServer` renaming `gh` to `github`, keeping one variable and setting another to `value`.
fn save(value: &str) -> McpCommand {
    McpCommand::SaveServer {
        previous_name: Some("gh".to_owned()),
        server: McpServerEdit {
            name: "github".to_owned(),
            kind: McpServerKind::Local,
            command: Some("/opt/homebrew/bin/npx".to_owned()),
            args: vec![
                "-y".to_owned(),
                "@modelcontextprotocol/server-github".to_owned(),
            ],
            url: None,
            env: vec![
                McpSecretEdit {
                    name: "GITHUB_PERSONAL_ACCESS_TOKEN".to_owned(),
                    value: McpSecretValue::Set(secret(value)),
                },
                McpSecretEdit {
                    name: "GITHUB_TOOLSETS".to_owned(),
                    value: McpSecretValue::Keep {},
                },
            ],
            headers: Vec::new(),
            enabled: true,
        },
    }
}

// The `SaveServer` golden holds the placeholder, since a real value cannot round-trip: it
// encodes as `[redacted]`. `save_server_with_a_real_value` checks one with a real value.
mod mcp_command {
    use super::*;

    golden! { McpCommand::name_of {
    SaveServer => save_server: super::save(clauseo_protocol::REDACTED_TEXT);
    RemoveServer => remove_server: McpCommand::RemoveServer { name: "github".to_owned() };
    SetServerEnabled => set_server_enabled: McpCommand::SetServerEnabled {
        name: "github".to_owned(),
        enabled: false,
    };
    } }
}

/// The view the screen reads, and each control in order.
#[test]
fn mcp_tools_view_and_controls() {
    snapshot!("mcp_controls", McpControl::ALL);
    round_trips(&McpControl::ALL);
    round_trips(&tools_view());
}

/// A command holding a real value encodes as the placeholder one does: `[redacted]`, never the
/// value, in the JSON and `Debug`; and the command decodes back holding the placeholder, which
/// `validate` refuses as a value.
#[test]
fn save_server_with_a_real_value() {
    let command = HostCommand::Mcp(save(TOKEN));
    let json = serde_json::to_string(&command).unwrap();
    assert!(!json.contains(TOKEN), "{json}");
    assert!(json.contains("[redacted]"), "{json}");
    assert!(!format!("{command:?}").contains(TOKEN));
    let envelope = |command: HostCommand| CommandEnvelope {
        command_id: fixtures::command_id(1),
        command,
    };
    assert!(envelope(command).validate().is_ok());
    let back: HostCommand = serde_json::from_str(&json).unwrap();
    assert!(
        envelope(back).validate().is_err(),
        "the placeholder passed as a value"
    );
}

/// A misshapen command fails to decode with one fixed message, quoting none of its input: not
/// a value in the wrong place, not an unknown field holding one.
#[test]
fn a_misshapen_command_quotes_nothing() {
    let shapes = [
        format!(
            r#"{{"mcp": {{"save_server": {{"previous_name": null, "server": {{"name": "x", "kind": "local", "env": [{{"name": "A", "value": {{"keep": "{TOKEN}"}}}}], "enabled": true}}}}}}}}"#
        ),
        format!(
            r#"{{"mcp": {{"save_server": {{"previous_name": null, "server": {{"name": "x", "kind": "local", "env": [{{"name": "A", "value": {{"sett": "{TOKEN}"}}}}], "enabled": true}}}}}}}}"#
        ),
        format!(
            r#"{{"mcp": {{"save_server": {{"previous_name": null, "server": {{"name": "x", "kind": "local", "{TOKEN}": 1, "enabled": true}}}}}}}}"#
        ),
        format!(r#"{{"mcp": {{"remove_server": {{"name": 7, "token": "{TOKEN}"}}}}}}"#),
    ];
    for text in shapes {
        let error = serde_json::from_str::<HostCommand>(&text)
            .unwrap_err()
            .to_string();
        assert!(!error.contains(TOKEN), "{error}");
    }
}

/// Each kind's fields only, the bounds, and the placeholder are refused as shape errors.
#[test]
fn shape_rules() {
    let envelope = |command: McpCommand| CommandEnvelope {
        command_id: fixtures::command_id(1),
        command: HostCommand::Mcp(command),
    };
    let base = || {
        let McpCommand::SaveServer { server, .. } = save(TOKEN) else {
            unreachable!("save makes a SaveServer")
        };
        server
    };
    let with = |edit: &dyn Fn(&mut McpServerEdit)| {
        let mut server = base();
        edit(&mut server);
        envelope(McpCommand::SaveServer {
            previous_name: None,
            server,
        })
        .validate()
    };
    assert!(with(&|_| {}).is_ok());
    assert!(with(&|s| s.url = Some("https://x.example".to_owned())).is_err());
    assert!(with(&|s| s.kind = McpServerKind::Remote).is_err());
    assert!(with(&|s| s.args = vec![String::new(); 129]).is_err());
    assert!(with(&|s| s.name = "n".repeat(257)).is_err());
    // Names the host refuses with its own codes still pass the shape check.
    assert!(with(&|s| s.name = String::new()).is_ok());
    assert!(with(&|s| s.name = "n".repeat(65)).is_ok());
    assert!(with(&|s| s.command = Some("npx".to_owned())).is_ok());
    let remote = |edit: &dyn Fn(&mut McpServerEdit)| {
        with(&|s| {
            s.kind = McpServerKind::Remote;
            s.command = None;
            s.args.clear();
            s.env.clear();
            s.url = Some("https://mcp.linear.app/mcp".to_owned());
            edit(s);
        })
    };
    assert!(remote(&|_| {}).is_ok());
    assert!(remote(&|s| s.env = base().env).is_err());
    assert!(remote(&|s| s.headers = base().env).is_ok());
    assert!(
        envelope(McpCommand::RemoveServer {
            name: "n".repeat(257)
        })
        .validate()
        .is_err()
    );
}
