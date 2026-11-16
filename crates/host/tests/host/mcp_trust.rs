//! The lawyer's MCP servers run without asking (`agent/mcp_trust.rs`): Claude gets `mcp__<name>`
//! for each server turned on in `mcp.servers` and none under the firm's `managed-mcp.json`; a
//! Codex approval of a listed server's tool, or of a search tool of Clauseo's own `case-law`, is
//! answered `allow_once` by the host, unseen, while case law's `recharge`, an unlisted or
//! turned-off server (`case-law-2` included), a look-alike shell command and a server's own
//! question still reach the lawyer.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{DomainEvent, StopReason, StreamId};
use serde_json::{Value, json};

use crate::permissions::answer_from;
use crate::profile_rig::{
    CLAUDE, CODEX, Wired, agent_lines, agents, codex_lines, settings, template, turn,
};
use crate::support::{Dir, Scenario, matter_with_thread};

/// `docs` and `case` turned on, `off` turned off; nothing launches them.
fn servers(dir: &Dir) -> Value {
    let bin = dir.folder("bin", &[]);
    let command = |name: &str| bin.join(name).display().to_string();
    json!({"mcp": {"servers": {
        "docs": {"command": command("docs"), "enabled": true},
        "case": {"command": command("case"), "enabled": true},
        "off": {"command": command("off")},
    }}})
}

/// [`servers`] with a lawyer's entries of the reserved names, turned on: they are left out.
fn servers_with_reserved(dir: &Dir) -> Value {
    let mut all = servers(dir);
    let bin = dir.folder("bin", &[]);
    for name in ["browser", "case-law", "Case-Law"] {
        all["mcp"]["servers"][name] =
            json!({"command": bin.join("x").display().to_string(), "enabled": true});
    }
    all
}

/// The settings for a Claude session: the servers, with web search and the browser off so
/// `allowedTools` holds only the servers' rules.
fn claude_settings(dir: &Dir) -> Value {
    let mut extra = servers_with_reserved(dir);
    extra["tools"] = json!({"web_search": false, "browser": false});
    extra
}

fn entry(name: &str) -> Value {
    json!({"name": name, "command": "{{any}}", "args": [], "env": "{{any}}"})
}

/// Claude gets one allow rule per server turned on, in name order, and none for `off` or for a
/// lawyer's entry of a reserved name (`browser`, `case-law`), which is not sent at all.
#[test]
fn claude_allows_the_listed_servers_it_gets() {
    let dir = Dir::new("mcp_trust_claude", &[]);
    let params = json!({"mcpServers": [entry("case"), entry("docs")],
        "_meta": {"claudeCode": {"options": {"allowedTools": ["mcp__case", "mcp__docs"]}}}});
    let claude = template(
        &dir,
        "trust-claude",
        &agent_lines("trust-claude", "session/new", &params, &[]),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({}))]),
        claude_settings(&dir),
    );
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| config);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    host.shutdown();
}

/// A server turned on but not sent gets no allow rule: one whose program is missing, and a
/// remote one to an agent that takes none (no `mcpCapabilities.http`). The trust is the servers
/// the session gets.
#[test]
fn claude_allows_only_the_servers_it_is_sent() {
    let dir = Dir::new("mcp_trust_claude_sent", &[]);
    let params = json!({"mcpServers": [entry("case"), entry("docs")],
        "_meta": {"claudeCode": {"options": {"allowedTools": ["mcp__case", "mcp__docs"]}}}});
    let claude = template(
        &dir,
        "trust-sent",
        &agent_lines("trust-sent", "session/new", &params, &[]),
    );
    let mut extra = claude_settings(&dir);
    let gone = dir.folder("bin", &[]).join("gone").display().to_string();
    extra["mcp"]["servers"]["gone"] = json!({"command": gone, "enabled": true});
    extra["mcp"]["servers"]["far"] =
        json!({"url": "https://tools.example.com/mcp", "enabled": true});
    settings(&dir, agents(&[(CLAUDE, &claude, json!({}))]), extra);
    let launcher = Wired::new(&dir);
    // `gone`'s program is not on this computer.
    let finds = |typed: &str| (!typed.ends_with("gone")).then(|| PathBuf::from(typed));
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        config.with_mcp_resolver(Arc::new(finds))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    host.shutdown();
}

/// A remote server sent to an agent that takes them (`mcpCapabilities.http`) joins the trust
/// like a local one: the session gets it and an allow rule for it.
#[test]
fn claude_allows_a_remote_server_it_is_sent() {
    let dir = Dir::new("mcp_trust_claude_remote", &[]);
    let far = json!({"type": "http", "name": "far", "url": "https://tools.example.com/mcp",
        "headers": []});
    let params = json!({"mcpServers": [entry("case"), entry("docs"), far],
        "_meta": {"claudeCode": {"options": {
            "allowedTools": ["mcp__case", "mcp__docs", "mcp__far"]}}}});
    let lines = agent_lines("trust-remote", "session/new", &params, &[]).replace(
        r#""agentCapabilities":{"loadSession":true}"#,
        r#""agentCapabilities":{"loadSession":true,"mcpCapabilities":{"http":true}}"#,
    );
    assert!(
        lines.contains("mcpCapabilities"),
        "the rig's initialize answer changed"
    );
    let claude = template(&dir, "trust-remote", &lines);
    let mut extra = claude_settings(&dir);
    extra["mcp"]["servers"]["far"] =
        json!({"url": "https://tools.example.com/mcp", "enabled": true});
    settings(&dir, agents(&[(CLAUDE, &claude, json!({}))]), extra);
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| config);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    host.shutdown();
}

/// Under the firm's `managed-mcp.json` no listed server goes, and none is allowed.
#[test]
fn claude_allows_none_under_the_firms_file() {
    let dir = Dir::new("mcp_trust_claude_managed", &[]);
    dir.set_managed_mcp();
    let params = json!({"mcpServers": [],
        "_meta": {"claudeCode": {"options": {"allowedTools": []}}}});
    let claude = template(
        &dir,
        "trust-managed",
        &agent_lines("trust-managed", "session/new", &params, &[]),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({}))]),
        claude_settings(&dir),
    );
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| config);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    host.shutdown();
}

/// The agent's tool call ids of the requests the lawyer saw, in order.
fn requested_tools(events: &crate::observe::Events) -> Vec<Option<String>> {
    let started: std::collections::BTreeMap<_, String> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ToolCallStarted(started) => {
                Some((started.tool_call, started.acp_id.to_string()))
            }
            _ => None,
        })
        .collect();
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PermissionRequested(requested) => Some(
                requested
                    .tool_call
                    .and_then(|tool| started.get(&tool).cloned()),
            ),
            _ => None,
        })
        .collect()
}

/// `codex-trusted-mcp`: the host answers the first approval of the `docs` call and of the
/// `case` call `allow_once` with nothing logged; the scenario checks each answer, so a request
/// the host answered that the lawyer should have seen (or the other way round) is a mismatch.
#[test]
fn codex_runs_listed_servers_tools_unasked_and_asks_for_the_rest() {
    let dir = Dir::new("mcp_trust_codex", &[]);
    let extra = servers(&dir);
    dir.set_settings(&[(CODEX, Scenario::Local("codex-trusted-mcp"))], extra);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find the judgment.");
    let mut answers = vec!["cancel"; 10];
    answers.push("decline");
    answer_from(&host, &mut events, thread, 0, &answers);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let seen = requested_tools(&events);
    let expected = [
        Some("tc-docs"), // the same call asked again: a server's own question
        None,            // docs standalone
        Some("tc-extra"),
        Some("tc-off"),
        Some("tc-law"), // no case-law entry was sent this session
        None,
        Some("tc-recharge"),
        Some("tc-law2"),
        None,
        Some("tc-shell"),
        None, // docs's question
    ];
    let seen: Vec<Option<&str>> = seen.iter().map(Option::as_deref).collect();
    assert_eq!(seen, expected, "{}", events.dump());
    host.shutdown();
}

/// Starts `scenario` for a Codex agent with the servers, `adjust` given the config, and checks
/// that `asked` requests reach the lawyer, each answered `cancel`, and the turn ends.
fn codex_all_ask(
    test: &str,
    scenario: &'static str,
    asked: usize,
    managed: bool,
    adjust: impl FnOnce(clauseo_host::HostConfig) -> clauseo_host::HostConfig,
) {
    let dir = Dir::new(test, &[]);
    if managed {
        dir.set_managed_mcp();
    }
    let extra = servers(&dir);
    dir.set_settings(&[(CODEX, Scenario::Local(scenario))], extra);
    let host = dir.start_with(0, adjust);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find the judgment.");
    answer_from(&host, &mut events, thread, 0, &vec!["cancel"; asked]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(requested_tools(&events).len(), asked, "{}", events.dump());
    host.shutdown();
}

/// Under the firm's `managed-mcp.json` a listed server's tools ask on Codex too.
#[test]
fn codex_trusts_no_listed_server_under_the_firms_file() {
    codex_all_ask(
        "mcp_trust_codex_managed",
        "codex-trusted-mcp-managed",
        2,
        true,
        |config| config,
    );
}

/// A failed folder check, where Claude would run in forced ask: Codex asks for the lawyer's
/// servers and case law too.
#[test]
fn codex_asks_for_every_mcp_tool_where_claude_would_be_forced_to_ask() {
    codex_all_ask(
        "mcp_trust_codex_unprotected",
        "codex-trusted-mcp-unprotected",
        4,
        false,
        clauseo_host::HostConfig::with_folder_check_failed,
    );
}

/// With Clauseo's `case-law` entry sent (the connector signed in, its relay open): the approval
/// of a `search_law` call is answered by the host, and so are two standalone approvals while two
/// `search_law` calls run at once (codex-acp's form for parallel calls), each taking one call;
/// a third standalone approval, with no call left, and `recharge` reach the lawyer.
#[test]
fn codex_runs_case_law_search_unasked_only_with_our_entry() {
    use crate::case_law::{Connector, wire};
    let connector = Connector::new(true);
    let dir = Dir::new("mcp_trust_case_law_on", &[]);
    let opts = json!([{"optionId": "allow_once", "name": "Allow", "kind": "allow_once"},
        {"optionId": "cancel", "name": "Cancel", "kind": "reject_once"}]);
    let call = |tc: &str, tool: &str| {
        json!({"update": {"update": {"sessionUpdate": "tool_call", "toolCallId": tc,
            "title": format!("mcp.case-law.{tool}"), "kind": "execute", "status": "in_progress",
            "rawInput": {"server": "case-law", "tool": tool, "arguments": {}}}}})
    };
    let ask = |id: &str, tc: &str, answer: &str| {
        [
            json!({"request": {"id": id, "method": "session/request_permission", "params": {
                "sessionId": "sess-1", "toolCall": {"toolCallId": tc, "kind": "execute",
                "status": "pending"}, "options": opts, "_meta": {"is_mcp_tool_approval": true}}}}),
            json!({"expect_response": {"id": id, "result": {"outcome": {"outcome": "selected",
                "optionId": answer}}}}),
        ]
    };
    let alone = |id: &str, n: u32, answer: &str| {
        let tc = format!("elicitation:sess-1:case-law:{n}");
        [
            json!({"request": {"id": id, "method": "session/request_permission", "params": {
                "sessionId": "sess-1", "toolCall": {"toolCallId": tc,
                "kind": "execute", "status": "pending", "title": "MCP tool call approval",
                "rawInput": {"serverName": "case-law",
                    "description": "Allow the case-law MCP server to run tool \"search_law\"?",
                    "schema": {"type": "object", "properties": {}}}},
                "options": opts, "_meta": {"is_mcp_tool_approval": true}}}}),
            json!({"expect_response": {"id": id, "result": {"outcome": {"outcome": "selected",
                "optionId": answer}}}}),
        ]
    };
    let mut added = vec![call("tc-law", "search_law")];
    added.extend(ask("perm-law", "tc-law", "allow_once"));
    added.push(call("tc-p1", "search_law"));
    added.push(call("tc-p2", "search_law"));
    added.extend(alone("perm-p1", 1, "allow_once"));
    added.extend(alone("perm-p2", 2, "allow_once"));
    added.extend(alone("perm-p3", 3, "cancel"));
    added.push(call("tc-recharge", "recharge"));
    added.extend(ask("perm-recharge", "tc-recharge", "cancel"));
    let base = codex_lines("cl-on", &json!({"cwd": "{{any}}"}));
    let mut lines: Vec<String> = base.lines().map(str::to_owned).collect();
    let prompt = lines
        .iter()
        .position(|line| line.contains("\"session/prompt\""))
        .unwrap();
    for (n, line) in added.iter().enumerate() {
        lines.insert(prompt + 1 + n, line.to_string());
    }
    let codex = template(&dir, "cl-on", &lines.join("\n"));
    settings(&dir, agents(&[(CODEX, &codex, json!({}))]), json!({}));
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        wire(config, Some(&connector.relays))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find the judgment.");
    answer_from(&host, &mut events, thread, 0, &["cancel", "cancel"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let seen = requested_tools(&events);
    let seen: Vec<Option<&str>> = seen.iter().map(Option::as_deref).collect();
    assert_eq!(seen, [None, Some("tc-recharge")], "{}", events.dump());
    host.shutdown();
}
