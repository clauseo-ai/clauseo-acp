//! Which servers run without asking: the settings list, turned on, never a reserved name, none
//! under the firm's `managed-mcp.json`; Claude's rules; exact attribution of Codex's requests;
//! forced ask first.

use std::collections::BTreeMap;

use clauseo_acp::ToolCall;
use clauseo_protocol::{CompactString, TextOrBlob, ToolCallPatch, ToolCallStatus, ToolKind};
use clauseo_settings::McpServerLaunch;
use serde_json::{Value, json};

use super::{CodexCalls, claude_rule, codex_call, is_trusted, never_asks, trusted_servers};
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
        Some(&json!([
            "mcp__docs",
            "mcp__case",
            "mcp__case-law__search_law",
            "mcp__case-law__read_document",
            "mcp__case-law__citator",
            "mcp__case-law__check_citations",
            "mcp__case-law__account"
        ]))
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
    assert_eq!(calls.call("tc-1"), Some(("docs", "search")));
    assert_eq!(calls.call("tc-shell"), None);
    assert_eq!(calls.call("tc-none"), None);
    assert_eq!(calls.call("tc-done"), None);
    calls.updated(
        "tc-1",
        &ToolCallPatch {
            status: Some(ToolCallStatus::Completed),
            ..ToolCallPatch::default()
        },
    );
    assert_eq!(calls.call("tc-1"), None);
}

/// Only Clauseo's own `case-law`, by exact name, and only its search and read tools, by exact
/// name, get the built-in's pass: not `recharge`, not an unknown tool, not a tool the host could
/// not read, and not a lawyer's server named like it. A settings entry of the reserved name is
/// never trusted or sent.
#[test]
fn only_the_built_in_case_laws_search_and_read_tools_never_ask() {
    for tool in [
        "search_law",
        "read_document",
        "citator",
        "check_citations",
        "account",
    ] {
        assert!(never_asks("case-law", Some(tool)), "{tool}");
        assert!(!never_asks("case", Some(tool)), "{tool}");
        assert!(!never_asks("case-law-2", Some(tool)), "{tool}");
    }
    for tool in [
        Some("recharge"),
        Some("Search_law"),
        Some("search_law "),
        Some("x"),
        None,
    ] {
        assert!(!never_asks("case-law", tool), "{tool:?}");
    }
    for other in [
        "Case-Law",
        "CASE-LAW",
        "case-law ",
        "case_law",
        "browser",
        "docs",
    ] {
        assert!(
            !never_asks(other, Some("search_law")),
            "{other:?} gets the pass"
        );
    }
    let servers = list(&[("case-law", true), ("Case-Law", true), ("case-law-2", true)]);
    assert_eq!(
        trusted_servers(&servers, McpControl::Clauseo),
        names(&["case-law-2"])
    );
}

/// A listed name holds only ASCII letters, digits, `_` and `-`
/// (`clauseo_settings`'s `leaf/mcp.rs` `mcp_name_ok`), so no `.` or `:` can make a title or an
/// id ambiguous; even so, the server is the input's own field, compared byte for byte.
#[test]
fn dots_colons_and_case_cannot_fake_a_server() {
    // The server and tool come from the input; the title must be exactly theirs.
    assert_eq!(
        codex_call("mcp.a.b.c", &json!({"server": "a.b", "tool": "c"})),
        Some(("a.b", "c"))
    );
    assert_eq!(
        codex_call("mcp.a.b.c", &json!({"server": "a", "tool": "b.c"})),
        Some(("a", "b.c"))
    );
    assert!(!is_trusted(&names(&["a"]), "a.b"));
    // Case-sensitive, byte for byte.
    assert_eq!(
        codex_call(
            "mcp.Docs.search",
            &json!({"server": "Docs", "tool": "search"})
        ),
        Some(("Docs", "search"))
    );
    assert!(!is_trusted(&names(&["docs"]), "Docs"));
    assert_eq!(
        codex_call(
            "mcp.Docs.search",
            &json!({"server": "docs", "tool": "search"})
        ),
        None
    );
    assert!(!is_trusted(&names(&["docs"]), "docs:1"));
    // No server field, or not a string: no pass.
    assert_eq!(
        codex_call("mcp.docs.search", &json!({"tool": "search", "server": 1})),
        None
    );
    assert_eq!(
        codex_call(
            "mcp.docs.search",
            &json!({"server": null, "tool": "search"})
        ),
        None
    );
}

/// Only an MCP call is kept: a dynamic tool (it has a `name`) or a shell command (its input is a
/// command), even with a server-like input and an MCP-like title, never is; nor is the same call
/// kept twice.
#[test]
fn only_mcp_calls_are_kept_once() {
    let mut calls = CodexCalls::default();
    let input = json!({"server": "docs", "tool": "search"});
    let mut named = call(
        "tc-dyn",
        "mcp.docs.search",
        Some(input.clone()),
        ToolCallStatus::InProgress,
    );
    named.tool_name = Some(CompactString::from("search"));
    calls.started(&named);
    calls.started(&call(
        "tc-shell",
        "mcp.docs.search",
        Some(json!({"command": ["mcp.docs.search"], "cwd": "/m", "server": "docs", "tool": "search"})),
        ToolCallStatus::Pending,
    ));
    assert_eq!(calls.len(), 0);
    let mcp = call(
        "tc-1",
        "mcp.docs.search",
        Some(input),
        ToolCallStatus::InProgress,
    );
    calls.started(&mcp);
    calls.started(&mcp);
    assert_eq!(calls.len(), 1);
}

/// A call is taken by the host's one answer: a second request naming it finds nothing.
#[test]
fn a_call_is_answered_once() {
    let mut calls = CodexCalls::default();
    let input = json!({"server": "docs", "tool": "search"});
    calls.started(&call(
        "tc-1",
        "mcp.docs.search",
        Some(input),
        ToolCallStatus::InProgress,
    ));
    assert_eq!(
        calls.take("tc-1"),
        Some(("docs".to_owned(), "search".to_owned()))
    );
    assert_eq!(calls.take("tc-1"), None);
}
