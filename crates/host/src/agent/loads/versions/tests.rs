//! The `clauseo-versions` entry's fill: the listener's port and its own token file, no entry
//! without a listener or under the firm's `managed-mcp.json`, the Windows variables, and
//! Claude's pre-allow rules.

use std::path::PathBuf;

use clauseo_acp::McpServer;

use super::{VersionsInputs, claude_allow, hook_servers, is_versions_rule, unasked_tool};
use crate::agent::loads::McpControl;

fn server(name: &str, args: &[&str], env: &[(&str, &str)]) -> McpServer {
    McpServer::Stdio {
        name: name.to_owned(),
        command: PathBuf::from("/app/clauseo-mcp"),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        env: env
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

/// The hook's servers as the profile offers them: `case-law`, then `clauseo-versions`.
fn hook() -> Vec<McpServer> {
    vec![
        server(
            "case-law",
            &["connector", "case-law"],
            &[("CLAUSEO_RELAY", "127.0.0.1:{relay_port}")],
        ),
        server(
            "clauseo-versions",
            &["connector", "clauseo-versions"],
            &[
                ("CLAUSEO_RELAY", "127.0.0.1:{versions_port}"),
                ("CLAUSEO_RELAY_TOKEN_FILE", "{versions_token_file}"),
            ],
        ),
    ]
}

fn token_file() -> PathBuf {
    PathBuf::from("/data/run/s-1/versions-token")
}

#[test]
fn an_open_listener_fills_the_entry_with_its_port_and_token_file() {
    let file = token_file();
    let windows_env = [("SystemRoot".to_owned(), r"C:\Windows".to_owned())];
    let inputs = VersionsInputs {
        listener: Some((61_235, &file)),
        control: McpControl::Clauseo,
        windows_env: &windows_env,
    };
    let (rest, entry) = hook_servers(hook(), &inputs);
    assert_eq!(rest, vec![hook().remove(0)]);
    let path = dunce::simplified(&file).to_str().unwrap().to_owned();
    assert_eq!(
        entry,
        Some(server(
            "clauseo-versions",
            &["connector", "clauseo-versions"],
            &[
                ("CLAUSEO_RELAY", "127.0.0.1:61235"),
                ("CLAUSEO_RELAY_TOKEN_FILE", &path),
                ("SystemRoot", r"C:\Windows"),
            ],
        ))
    );
}

/// No listener, or the firm's `managed-mcp.json`: no entry, and none of the hook's goes either.
#[test]
fn no_entry_without_a_listener_or_under_managed_mcp() {
    let file = token_file();
    for inputs in [
        VersionsInputs {
            listener: None,
            control: McpControl::Clauseo,
            windows_env: &[],
        },
        VersionsInputs {
            listener: Some((61_235, &file)),
            control: McpControl::Managed,
            windows_env: &[],
        },
    ] {
        let (rest, entry) = hook_servers(hook(), &inputs);
        assert_eq!((rest.len(), entry), (1, None), "{inputs:?}");
    }
}

/// An entry with a token the host cannot fill (the relay's) is dropped, not sent half filled.
#[test]
fn an_entry_with_another_token_is_left_out() {
    let file = token_file();
    let hook = vec![server(
        "clauseo-versions",
        &["connector", "clauseo-versions"],
        &[("CLAUSEO_RELAY_TOKEN_FILE", "{relay_token_file}")],
    )];
    let inputs = VersionsInputs {
        listener: Some((61_235, &file)),
        control: McpControl::Clauseo,
        windows_env: &[],
    };
    assert_eq!(hook_servers(hook, &inputs), (Vec::new(), None));
}

#[test]
fn claude_gets_one_rule_per_tool_and_only_the_two_tools_go_unasked() {
    assert_eq!(
        claude_allow().collect::<Vec<_>>(),
        [
            "mcp__clauseo-versions__list_versions",
            "mcp__clauseo-versions__read_version"
        ]
    );
    assert!(unasked_tool("list_versions") && unasked_tool("read_version"));
    for other in ["write_file", "List_versions", "read_version ", ""] {
        assert!(!unasked_tool(other), "{other}");
    }
    assert!(is_versions_rule("mcp__clauseo-versions"));
    assert!(is_versions_rule("mcp__clauseo-versions__anything"));
    assert!(!is_versions_rule("mcp__clauseo-versions-2"));
    assert!(!is_versions_rule("mcp__case-law__search_law"));
}

/// Claude's `allowedTools` gains the two rules only with our entry, and loses any hook rule for
/// a `clauseo-versions` server either way; forced ask withholds every rule, these included.
#[test]
fn claude_pre_allows_the_tools_only_with_our_entry_and_never_in_forced_ask() {
    use serde_json::{Value, json};

    use crate::agent::loads::browser::Switch;
    use crate::agent::loads::profile::{ClaudeKeys, Folders, Login, claude_options};
    use crate::versions::VersionsTool;
    let allowed = |versions: VersionsTool, forced_ask: bool| {
        let mut options = json!({"allowedTools": [
            "mcp__docs", "mcp__clauseo-versions", "mcp__clauseo-versions__x"
        ]})
        .as_object()
        .cloned()
        .unwrap();
        let keys = ClaudeKeys {
            web_search: Switch {
                on: false,
                locked: false,
            },
            login: Login::FirstParty,
            folders: Folders {
                work: "/w",
                notes: "/n",
                connectors: "",
                browser: "",
            },
            seed_sites: &[],
            forced_ask,
            data: &crate::agent::data_fence::LaunchDeny::default(),
            case_law: false,
            versions,
            browser: false,
            sandbox: None,
            matter: None,
            trusted_servers: &[],
        };
        claude_options(&mut options, &keys);
        options.get("allowedTools").cloned()
    };
    assert_eq!(
        allowed(VersionsTool::Offered, false),
        Some(json!([
            "mcp__docs",
            "mcp__clauseo-versions__list_versions",
            "mcp__clauseo-versions__read_version"
        ]))
    );
    assert_eq!(
        allowed(VersionsTool::Absent, false),
        Some(json!(["mcp__docs"]))
    );
    assert_eq!(allowed(VersionsTool::Offered, true), None::<Value>);
}
