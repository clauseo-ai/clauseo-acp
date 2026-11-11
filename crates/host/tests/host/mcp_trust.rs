//! The lawyer's MCP servers run without asking (`agent/mcp_trust.rs`): Claude gets `mcp__<name>`
//! for each server turned on in `mcp.servers` and none under the firm's `managed-mcp.json`; a
//! Codex approval of a listed server's tool, or of a search tool of Clauseo's own `case-law`, is
//! answered `allow_once` by the host, unseen, while case law's `recharge`, an unlisted or
//! turned-off server (`case-law-2` included), a look-alike shell command and a server's own
//! question still reach the lawyer.

use std::sync::Arc;

use clauseo_protocol::{DomainEvent, StopReason, StreamId};
use serde_json::{Value, json};

use crate::permissions::answer_from;
use crate::profile_rig::{CLAUDE, CODEX, Wired, agent_lines, agents, settings, template, turn};
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

/// The settings for a Claude session: the servers, with web search and the browser off so
/// `allowedTools` holds only the servers' rules.
fn claude_settings(dir: &Dir) -> Value {
    let mut extra = servers(dir);
    extra["tools"] = json!({"web_search": false, "browser": false});
    extra
}

fn entry(name: &str) -> Value {
    json!({"name": name, "command": "{{any}}", "args": [], "env": []})
}

/// Claude gets one allow rule per server turned on, in name order, and none for `off`.
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

/// `codex-trusted-mcp`: the host answers `docs` (correlated and standalone), `case` and
/// `case-law`'s `search_law` (both forms) `allow_once` with nothing logged; the scenario checks
/// each answer, so a request the host answered that the lawyer should have seen (or the other
/// way round) is a mismatch.
#[test]
fn codex_runs_listed_servers_tools_unasked_and_asks_for_the_rest() {
    let dir = Dir::new("mcp_trust_codex", &[]);
    let extra = servers(&dir);
    dir.set_settings(&[(CODEX, Scenario::Local("codex-trusted-mcp"))], extra);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find the judgment.");
    answer_from(
        &host,
        &mut events,
        thread,
        0,
        &[
            "cancel", "cancel", "cancel", "cancel", "cancel", "cancel", "cancel", "decline",
        ],
    );
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let seen = requested_tools(&events);
    assert_eq!(seen.len(), 8, "{}", events.dump());
    assert_eq!(seen[0].as_deref(), Some("tc-extra"));
    assert_eq!(seen[1].as_deref(), Some("tc-off"));
    assert_eq!(seen[2].as_deref(), Some("tc-recharge"));
    assert_eq!(seen[3], None, "recharge's standalone request");
    assert_eq!(seen[4].as_deref(), Some("tc-law2"));
    assert_eq!(seen[5], None, "case-law-2's standalone request");
    assert_eq!(seen[6].as_deref(), Some("tc-shell"));
    assert!(
        !seen
            .iter()
            .any(|tool| matches!(tool.as_deref(), Some("tc-docs" | "tc-case" | "tc-law"))),
        "{}",
        events.dump()
    );
    host.shutdown();
}
