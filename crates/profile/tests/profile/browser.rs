//! The browser's parts of the profile: agent-browser's rules for Claude and the `{browser}`
//! part each session gets.

use clauseo_profile::instructions::{TOOLS, word_count};
use clauseo_profile::placeholders::{self, BROWSER, IN_INSTRUCTIONS, MATTER_NOTES, WORK_FOLDER};
use serde_json::Value;

use super::profile;

/// agent-browser's rules for Claude: the page subcommands allowed, the options that reach the
/// lawyer's own browser or swap Clauseo's settings denied, everything else asking. The profile
/// never sends them, with the browser or without: the host adds them per session.
#[test]
fn agent_browser_rules_are_the_hosts_per_session() {
    use clauseo_profile::claude::{
        AGENT_BROWSER_ALLOWED, AGENT_BROWSER_DENIED, agent_browser_allowed, agent_browser_denied,
    };
    let allowed: Vec<String> = agent_browser_allowed().collect();
    assert_eq!(allowed.len(), 2 * AGENT_BROWSER_ALLOWED.len());
    for rule in [
        "Bash(agent-browser open:*)",
        "Bash(agent-browser --json snapshot:*)",
        "Bash(agent-browser snapshot:*)",
        "Bash(agent-browser read:*)",
    ] {
        assert!(allowed.iter().any(|a| a == rule), "{rule}");
    }
    // eval runs page JavaScript, and pdf and download write to a path the agent names: each asks.
    let rules = ["eval", "pdf", "download"].map(|sub| format!("Bash(agent-browser {sub}:*)"));
    let none = rules.iter().all(|rule| !allowed.contains(rule));
    assert!(none, "{rules:?}");
    let ask: Vec<String> = clauseo_profile::claude::agent_browser_ask().collect();
    assert_eq!(ask.len(), 2 * 6 + 12);
    for rule in [
        "Bash(agent-browser wait*-f*)",
        "Bash(agent-browser get*cdp-url*)",
        "Bash(agent-browser eval*)",
        "Bash(agent-browser --json eval*)",
        "Bash(agent-browser pdf*)",
        "Bash(agent-browser download*)",
        "Bash(agent-browser *file://*)",
        "Bash(agent-browser * upload*)",
        "Bash(agent-browser * state load*)",
    ] {
        assert!(ask.iter().any(|a| a == rule), "{rule}");
    }
    for asks in [
        "upload", "eval", "state", "cookies", "storage", "network", "set", "install",
    ] {
        assert!(
            !allowed.iter().any(|a| a.contains(&format!(" {asks}:"))),
            "{asks}"
        );
    }
    let denied: Vec<String> = agent_browser_denied().collect();
    assert_eq!(denied.len(), AGENT_BROWSER_DENIED.len() + 2 * 2);
    for rule in [
        "Bash(agent-browser *--profile*)",
        "Bash(agent-browser *--auto-connect*)",
        "Bash(agent-browser *--cdp*)",
        "Bash(agent-browser *--executable-path*)",
        "Bash(agent-browser *--headed*)",
        "Bash(agent-browser *--config*)",
        "Bash(agent-browser *--session*)",
        "Bash(agent-browser *--state*)",
        "Bash(agent-browser *--extension*)",
        "Bash(agent-browser state*)",
        "Bash(agent-browser connect*)",
        "Bash(agent-browser --json state*)",
        "Bash(agent-browser *--args*)",
    ] {
        assert!(denied.iter().any(|d| d == rule), "{rule}");
    }
    let (with, _) = profile().with_tool_skills(None, true);
    for options in [with.claude_options(), profile().claude_options()] {
        let meta = Value::Object(options.meta_fragment()).to_string();
        assert!(!meta.contains("agent-browser"), "{meta}");
    }
}

/// `{browser}` sits once in the tools part, and only the part a session with the browser gets
/// names `agent-browser`; the others say there is no browser, the not-installed one why.
#[test]
fn only_the_browser_part_names_agent_browser() {
    use clauseo_profile::instructions::{BrowserPart, browser_part, browser_part_for};
    assert_eq!(TOOLS.matches(BROWSER).count(), 1);
    let text = profile().instructions();
    assert!(!text.contains("agent-browser"));
    let fill = |part| {
        placeholders::fill(
            &text,
            &IN_INSTRUCTIONS,
            &[
                (WORK_FOLDER, "/w"),
                (MATTER_NOTES, "/n"),
                (BROWSER, browser_part(part)),
            ],
        )
        .unwrap()
    };
    let on = fill(BrowserPart::On);
    assert!(on.contains("over 125 characters, use `curl`. For a page that needs a real"));
    assert!(on.contains("over 125 characters, `agent-browser read` gives the page's text."));
    for rule in [
        "`agent-browser`",
        "`AGENT_BROWSER_SESSION`",
        "`--headed`",
        "`state save`",
        "named in this conversation",
        "confidential matter text",
        "downloads folder",
        "chained only with other `agent-browser` commands",
        "no sandbox options such as allowed domains",
        "`agent-browser close`",
    ] {
        assert!(on.contains(rule), "{rule}");
    }
    for part in [
        BrowserPart::Off,
        BrowserPart::NotInstalled,
        BrowserPart::ClaudeOnly,
    ] {
        let off = fill(part);
        assert!(!off.contains("agent-browser") && !off.contains("AGENT_BROWSER"));
        assert!(off.contains("You have no browser in this session"));
    }
    assert!(fill(BrowserPart::NotInstalled).contains("found no Google Chrome"));
    // A Codex session with the browser is told to use the `browser` MCP entry's tools, never the
    // shell command or its variables, and never to escalate; Claude never gets Codex's part.
    let codex_on = browser_part_for(BrowserPart::On, true);
    for rule in [
        "use your browser tools, the ones from the `browser` MCP server",
        "never a browser command in your shell",
        "confidential matter text",
        "close tool",
    ] {
        assert!(codex_on.contains(rule), "{rule}: {codex_on}");
    }
    for word in [
        "agent-browser",
        "AGENT_BROWSER",
        "sandbox_permissions",
        "require_escalated",
        "prefix_rule",
    ] {
        assert!(!codex_on.contains(word), "{word}: {codex_on}");
    }
    assert_eq!(
        browser_part_for(BrowserPart::On, false),
        browser_part(BrowserPart::On)
    );
    for part in [
        BrowserPart::Off,
        BrowserPart::NotInstalled,
        BrowserPart::ClaudeOnly,
    ] {
        assert_eq!(browser_part_for(part, true), browser_part(part));
        assert!(!browser_part_for(part, true).contains("agent-browser"));
    }
    assert!(word_count(browser_part(BrowserPart::NotInstalled)) <= 50);
}
