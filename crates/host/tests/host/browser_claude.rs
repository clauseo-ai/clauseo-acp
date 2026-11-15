//! Claude's browser is the guarded `browser` MCP entry, as Codex's is
//! (`clauseo_profile::servers::browser_mcp`): off Windows a Claude session gets the entry filled,
//! `mcp__browser` in its `allowedTools` (no prompt for ordinary browsing), Claude's own entry
//! line in its instructions and `Available` in its header; on Windows none of it and the
//! no-browser sentence. A matter Claude's sandbox cannot name (a `*` in its folder) leaves the
//! sandbox off, so that session gets no entry, no allow rule, no browser in its header, the
//! no-browser sentence and one notice saying why (it also asks before commands, which the same
//! notice says; `claude_mode.rs` checks the mode).

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_profile::instructions::{BrowserPart, BrowserRoute, browser_part, browser_part_for};
use clauseo_protocol::{
    AgentKind, ClauseoTool, DomainEvent, NoticeKind, StopReason, StreamId, ThreadId,
    ToolAvailability, ToolState,
};
use serde_json::{Value, json};

use crate::profile_rig::{
    CLAUDE, Hook, Wired, agent_lines, agents, browser_server, filled_browser, first_frame_tools,
    settings, template,
};
#[cfg(unix)]
use crate::support::matter_with_thread;
use crate::support::{Dir, Running};

/// The instructions the hook sends: the browser part goes after this sentence.
const APPEND: &str = "Read pages.{browser}";

/// The `session/new` params of a Claude session: `servers`, `allowed` as its `allowedTools`,
/// and `part` filling `{browser}`.
fn params(servers: &Value, allowed: &Value, part: &str) -> Value {
    json!({"mcpServers": servers, "_meta": {
        "systemPrompt": {"append": format!("Read pages.{part}")},
        "claudeCode": {"options": {"allowedTools": allowed}},
    }})
}

/// A host whose one Claude agent expects `params`, wired with the browser (the hook offers
/// Claude the entry, a proxy on 8123, Chrome at `/chrome`), web search off. With `no_sandbox`
/// the agent also expects the switch to `acceptEdits` before the prompt, as a session whose
/// sandbox is off gets (`Auto::NoSandbox`).
fn host(dir: &Dir, name: &str, params: &Value, no_sandbox: bool) -> Running {
    let mut lines: Vec<String> = agent_lines(name, "session/new", params, &[])
        .lines()
        .map(str::to_owned)
        .collect();
    if no_sandbox {
        let switch = [
            json!({"expect": {"method": "session/set_mode", "as": "to-edits",
                "params": {"sessionId": "sess-1", "modeId": "acceptEdits"}}}),
            json!({"respond": {"to": "to-edits", "result": {}}}),
        ];
        // After `session/new`'s answer, the fifth line.
        lines.splice(5..5, switch.map(|line| line.to_string()));
    }
    let claude = template(dir, name, &lines.join("\n"));
    settings(
        dir,
        agents(&[(CLAUDE, &claude, json!({}))]),
        json!({"tools": {"browser": true, "web_search": false}}),
    );
    let hook = Hook {
        meta: json!({"systemPrompt": {"append": APPEND}}),
        servers: vec![browser_server()],
        ..Hook::default()
    };
    let launcher = Wired::new(dir);
    dir.start_launching(0, launcher, |config| {
        config
            .with_profile_hook(Box::new(hook))
            .with_egress_proxy(Arc::new(clauseo_host::TestProxy(8123)))
            .with_browser_executable(Some(PathBuf::from("/chrome")))
    })
}

/// Runs one prompt on `thread` and returns its `SandboxOff` notices and its header's
/// browser state.
fn turn(host: &Running, thread: ThreadId) -> (usize, Option<ToolState>) {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, "Go.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let notices = events
        .events()
        .filter(|event| {
            matches!(event, DomainEvent::Notice(notice)
                if notice.kind == Some(NoticeKind::SandboxOff))
        })
        .count();
    let browser = first_frame_tools(host, thread)
        .into_iter()
        .find(|state| state.tool == ClauseoTool::Browser);
    (notices, browser)
}

#[test]
fn claude_gets_the_guarded_entry_unasked_off_windows() {
    let dir = Dir::new("browser_claude", &[]);
    let windows = clauseo_profile::servers::no_browser(Some(AgentKind::Claude));
    let expected = if windows {
        params(&json!([]), &json!([]), browser_part(BrowserPart::Off))
    } else {
        let part = browser_part_for(BrowserPart::On, BrowserRoute::ClaudeEntry);
        params(&json!([filled_browser()]), &json!(["mcp__browser"]), part)
    };
    let host = host(&dir, "bcl-on", &expected, false);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    let (notices, browser) = turn(&host, thread);
    host.shutdown();
    assert_eq!(notices, 0);
    if windows {
        assert_eq!(browser, None);
    } else {
        let browser = browser.expect("the header shows the browser");
        assert_eq!(browser.availability, ToolAvailability::Available);
    }
}

/// A `*` in the matter's folder leaves Claude's sandbox off (`Off::Wildcard`), so the entry is
/// dropped: no server, no allow rule, no browser in the header, the no-browser sentence, and one
/// notice. Unix only: Windows names hold no `*`, and Claude has no browser there anyway.
#[cfg(unix)]
#[test]
fn a_claude_session_without_its_sandbox_gets_no_browser_and_one_notice() {
    let dir = Dir::new("browser_claude_no_sandbox", &[]);
    let expected = params(&json!([]), &json!([]), browser_part(BrowserPart::Off));
    let host = host(&dir, "bcl-off", &expected, true);
    let (_, thread, _) = matter_with_thread(&dir, &host, "A*", &[("brief.txt", "x\n")], CLAUDE);
    let (notices, browser) = turn(&host, thread);
    host.shutdown();
    assert_eq!(notices, 1);
    assert_eq!(browser, None);
}
