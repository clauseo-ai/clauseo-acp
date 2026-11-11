//! Which servers run without asking: the settings list, turned on, never a reserved name, none
//! under the firm's `managed-mcp.json`; Claude's rules; exact attribution of Codex's requests;
//! forced ask first.

use std::collections::BTreeMap;

use clauseo_acp::ToolCall;
use clauseo_protocol::{CompactString, TextOrBlob, ToolCallPatch, ToolCallStatus, ToolKind};
use clauseo_settings::McpServerLaunch;
use serde_json::{Value, json};

use super::{
    CodexCalls, claude_rule, codex_call, codex_standalone_server, is_trusted, trusted_servers,
};
use crate::agent::loads::McpControl;

fn launch(enabled: bool) -> McpServerLaunch {
    McpServerLaunch {
        command: "/bin/server".to_owned(),
        args: Vec::new(),
        env: BTreeMap::new(),
        enabled,
    }
}

fn list(entries: &[(&str, bool)]) -> BTreeMap<String, McpServerLaunch> {
    entries
        .iter()
        .map(|(name, enabled)| ((*name).to_owned(), launch(*enabled)))
        .collect()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn only_turned_on_unreserved_servers_are_trusted_and_none_under_the_firms_file() {
    let servers = list(&[
        ("docs", true),
        ("off", false),
        ("case", true),
        ("Browser", true),
        ("CASE-LAW", true),
    ]);
    assert_eq!(
        trusted_servers(&servers, McpControl::Clauseo),
        names(&["case", "docs"])
    );
    assert_eq!(
        trusted_servers(&servers, McpControl::Managed),
        Vec::<String>::new()
    );
    assert_eq!(
        trusted_servers(&BTreeMap::new(), McpControl::Clauseo),
        Vec::<String>::new()
    );
}

#[test]
fn trust_matches_the_exact_name_only() {
    let trusted = names(&["case", "docs"]);
    assert!(is_trusted(&trusted, "case"));
    assert!(is_trusted(&trusted, "docs"));
    for other in [
        "case-law", "Case", "cas", "case ", " case", "docs2", "", "off",
    ] {
        assert!(!is_trusted(&trusted, other), "{other:?} is trusted");
    }
    // A reserved name is never trusted, even should it reach the list.
    let reserved = names(&["browser", "case-law"]);
    assert!(!is_trusted(&reserved, "browser"));
    assert!(!is_trusted(&reserved, "case-law"));
}

#[test]
fn claudes_rule_covers_one_server() {
    assert_eq!(claude_rule("docs"), "mcp__docs");
    assert_eq!(claude_rule("my-tools_2"), "mcp__my-tools_2");
}

/// The rules go into `allowedTools`, before case law's, without repeats; forced ask sends no
/// `allowedTools` at all, so the `mcp__*` ask rule holds for every server.
#[test]
fn claude_gets_allow_rules_unless_forced_ask() {
    use crate::agent::loads::browser::Switch;
    use crate::agent::loads::profile::{ClaudeKeys, Folders, Login, claude_options};
    let options = |trusted: &[String], case_law: bool, forced_ask: bool| {
        let mut options = json!({"allowedTools": ["mcp__docs"]})
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
            case_law,
            browser: false,
            sandbox: None,
            matter: None,
            trusted_servers: trusted,
        };
        claude_options(&mut options, &keys);
        options
    };
    let trusted = names(&["case", "docs"]);
    let open = options(&trusted, true, false);
    assert_eq!(
        open.get("allowedTools"),
        Some(&json!(["mcp__docs", "mcp__case", "mcp__case-law"]))
    );
    let none = options(&[], false, false);
    assert_eq!(none.get("allowedTools"), Some(&json!(["mcp__docs"])));
    let forced = options(&trusted, true, true);
    assert_eq!(forced.get("allowedTools"), None::<&Value>);
    let ask = &forced["settings"]["permissions"]["ask"];
    assert!(ask.as_array().unwrap().contains(&json!("mcp__*")), "{ask}");
}

#[test]
fn a_codex_call_needs_its_title_and_input_to_agree() {
    let input = json!({"server": "docs", "tool": "search", "arguments": {"q": "x"}});
    assert_eq!(
        codex_call("mcp.docs.search", &input),
        Some(("docs", "search"))
    );
    // A tool name with dots still parses: the server comes from the input, not the title.
    let dotted = json!({"server": "docs", "tool": "a.b", "arguments": {}});
    assert_eq!(codex_call("mcp.docs.a.b", &dotted), Some(("docs", "a.b")));
    // Title and input disagree, or one is missing.
    assert_eq!(
        codex_call(
            "mcp.case-law.search",
            &json!({"server": "case", "tool": "law.search"})
        ),
        None
    );
    assert_eq!(
        codex_call(
            "mcp.case.law.search",
            &json!({"server": "case-law", "tool": "search"})
        ),
        None
    );
    assert_eq!(
        codex_call(
            "mcp.docs.search",
            &json!({"server": "docs", "tool": "other"})
        ),
        None
    );
    assert_eq!(
        codex_call("mcp.docs.search", &json!({"tool": "search"})),
        None
    );
    assert_eq!(
        codex_call("mcp.docs.search", &json!({"server": "docs"})),
        None
    );
    assert_eq!(
        codex_call("mcp.docs.", &json!({"server": "docs", "tool": ""})),
        None
    );
    assert_eq!(
        codex_call("mcp..search", &json!({"server": "", "tool": "search"})),
        None
    );
    assert_eq!(codex_call("docs.search", &input), None);
    assert_eq!(codex_call("mcp.docs.search ", &input), None);
    // A shell command whose text looks like an MCP title: its input is a command.
    assert_eq!(
        codex_call(
            "mcp.docs.search",
            &json!({"command": "mcp.docs.search", "cwd": "/m"})
        ),
        None
    );
    assert_eq!(
        codex_call(
            "mcp.docs.search",
            &json!({"command": ["x"], "server": "docs", "tool": "search"})
        ),
        None
    );
    assert_eq!(
        codex_call("mcp.docs.search", &json!(["docs", "search"])),
        None
    );
}

#[test]
fn a_standalone_codex_approval_names_its_server_twice() {
    let input = |server: &str| {
        json!({"serverName": server, "description": "Allow the docs MCP server to run tool \"search\"?",
            "schema": {"type": "object", "properties": {}}})
    };
    let docs = input("docs");
    assert_eq!(
        codex_standalone_server("sess-1", "elicitation:sess-1:docs:0", Some(&docs)),
        Some("docs")
    );
    assert_eq!(
        codex_standalone_server("sess-1", "elicitation:sess-1:docs:17", Some(&docs)),
        Some("docs")
    );
    for id in [
        "elicitation:sess-2:docs:0",
        "elicitation:sess-1:docs-2:0",
        "elicitation:sess-1:doc:0",
        "elicitation:sess-1:docs:",
        "elicitation:sess-1:docs:1a",
        "elicitation:sess-1:docs",
        "elicitation-sess-1:docs:0",
        "tc-1",
    ] {
        assert_eq!(
            codex_standalone_server("sess-1", id, Some(&docs)),
            None,
            "{id}"
        );
    }
    // The id's server and `serverName` must be the same: `case` never stands for `case-law`.
    let case = input("case");
    assert_eq!(
        codex_standalone_server("sess-1", "elicitation:sess-1:case-law:0", Some(&case)),
        None
    );
    let law = input("case-law");
    assert_eq!(
        codex_standalone_server("sess-1", "elicitation:sess-1:case:0", Some(&law)),
        None
    );
    assert_eq!(
        codex_standalone_server("sess-1", "elicitation:sess-1:docs:0", None),
        None
    );
    assert_eq!(
        codex_standalone_server("sess-1", "elicitation:sess-1::0", Some(&input(""))),
        None
    );
}

fn call(id: &str, title: &str, input: Option<Value>, status: ToolCallStatus) -> ToolCall {
    ToolCall {
        acp_id: CompactString::from(id),
        title: CompactString::from(title),
        kind: ToolKind::Execute,
        tool_name: None,
        status,
        locations: Vec::new(),
        raw_input: input.map(|input| TextOrBlob::Inline(input.to_string())),
        raw_output: None,
        content: Vec::new(),
    }
}

#[test]
fn codex_calls_keep_unfinished_mcp_calls_by_id() {
    let mut calls = CodexCalls::default();
    let input = json!({"server": "docs", "tool": "search", "arguments": {}});
    calls.started(&call(
        "tc-1",
        "mcp.docs.search",
        Some(input.clone()),
        ToolCallStatus::InProgress,
    ));
    calls.started(&call(
        "tc-shell",
        "mcp.docs.search",
        Some(json!({"command": "mcp.docs.search", "cwd": "/m"})),
        ToolCallStatus::Pending,
    ));
    calls.started(&call(
        "tc-none",
        "mcp.docs.search",
        None,
        ToolCallStatus::Pending,
    ));
    calls.started(&call(
        "tc-done",
        "mcp.docs.search",
        Some(input),
        ToolCallStatus::Completed,
    ));
    assert_eq!(calls.server("tc-1"), Some("docs"));
    assert_eq!(calls.server("tc-shell"), None);
    assert_eq!(calls.server("tc-none"), None);
    assert_eq!(calls.server("tc-done"), None);
    calls.updated(
        "tc-1",
        &ToolCallPatch {
            status: Some(ToolCallStatus::Completed),
            ..ToolCallPatch::default()
        },
    );
    assert_eq!(calls.server("tc-1"), None);
}
