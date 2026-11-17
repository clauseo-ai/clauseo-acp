//! The browser's parts of the profile: agent-browser's rules for Claude and the `{browser}`
//! part each session gets.

use clauseo_profile::instructions::{TOOLS, word_count};
use clauseo_profile::placeholders::{self, BROWSER, IN_INSTRUCTIONS, MATTER_NOTES, WORK_FOLDER};
use serde_json::Value;

use super::profile;

/// agent-browser's rules for Claude: the page subcommands allowed; the options that reach the
/// lawyer's own browser or swap Clauseo's settings, and the commands that read or write a local
/// file or run page JavaScript, denied; a few options asking. The profile
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
    // eval runs page JavaScript, and pdf and download write to a path the agent names: none is
    // allowed, each is refused.
    let rules = ["eval", "pdf", "download"].map(|sub| format!("Bash(agent-browser {sub}:*)"));
    let none = rules.iter().all(|rule| !allowed.contains(rule));
    assert!(none, "{rules:?}");
    let ask: Vec<String> = clauseo_profile::claude::agent_browser_ask().collect();
    assert_eq!(
        ask,
        [
            "Bash(agent-browser *--init-script*)",
            "Bash(agent-browser *--enable*)",
            "Bash(agent-browser *-p *)",
            "Bash(agent-browser *file://*)",
        ]
    );
    for asks in [
        "upload", "eval", "state", "cookies", "storage", "network", "set", "install",
    ] {
        assert!(
            !allowed.iter().any(|a| a.contains(&format!(" {asks}:"))),
            "{asks}"
        );
    }
    let denied: Vec<String> = agent_browser_denied().collect();
    assert_eq!(denied.len(), AGENT_BROWSER_DENIED.len() + 2 * 2 + 2 * 6 + 8);
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
        "Bash(agent-browser wait*-f*)",
        "Bash(agent-browser get*cdp-url*)",
        "Bash(agent-browser eval*)",
        "Bash(agent-browser --json eval*)",
        "Bash(agent-browser pdf*)",
        "Bash(agent-browser --json pdf*)",
        "Bash(agent-browser download*)",
        "Bash(agent-browser --json download*)",
        "Bash(agent-browser upload*)",
        "Bash(agent-browser * eval*)",
        "Bash(agent-browser * pdf*)",
        "Bash(agent-browser * download*)",
        "Bash(agent-browser * upload*)",
        "Bash(agent-browser * state load*)",
        "Bash(agent-browser * state save*)",
        "Bash(agent-browser * wait *-f*)",
        "Bash(agent-browser *cdp-url*)",
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
    assert!(on.contains("with your web fetch tool. For a page that needs a real"));
    assert!(on.contains("over 125 characters, use `agent-browser read` or `curl`."));
    assert!(on.contains("`agent-browser eval`, `pdf` and `download` are off"));
    // Upload is always refused (`claude.rs`'s deny rules), so the part says nothing of it.
    assert!(!on.contains("Upload"));
    for rule in [
        "`agent-browser`",
        "`AGENT_BROWSER_SESSION`",
        "`--headed`",
        "`state save`",
        "confidential matter text",
        "downloads folder",
        "chained only with other `agent-browser` commands",
        "no sandbox options such as allowed domains",
        "`agent-browser close`",
    ] {
        assert!(on.contains(rule), "{rule}");
    }
    for part in [BrowserPart::Off, BrowserPart::NotInstalled] {
        let off = fill(part);
        assert!(!off.contains("agent-browser") && !off.contains("AGENT_BROWSER"));
        assert!(off.contains("You have no browser in this session"));
    }
    assert!(fill(BrowserPart::NotInstalled).contains("found no Google Chrome"));
    // A Codex session with the browser is told to use the `browser` MCP entry's tools, never the
    // shell command or its variables, and never to escalate; Claude never gets Codex's part.
    let codex_on = browser_part_for(BrowserPart::On, true);
    for rule in [
        "use the `browser` MCP server's tools",
        "never a browser command in your shell",
        "confidential matter text",
        "The eval tool is off",
        "Save a file from the web by clicking its link in the browser.",
        // A certificate warning: the lawyer hears it plainly, and no other route is tried.
        "Certificate error: stop, tell the lawyer the site's certificate couldn't be verified;",
        "never retry or suggest going past the warning.",
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
    for part in [BrowserPart::Off, BrowserPart::NotInstalled] {
        assert_eq!(browser_part_for(part, true), browser_part(part));
        assert!(!browser_part_for(part, true).contains("agent-browser"));
    }
    assert!(word_count(browser_part(BrowserPart::NotInstalled)) <= 50);
}

/// Every Codex session is told its shell has no internet, whatever its browser part (the line
/// sits outside `{browser}`): Clauseo declines Codex's downloads in the shell. Claude never is.
#[test]
fn every_codex_session_is_told_its_shell_is_offline() {
    use clauseo_profile::instructions::CODEX_SHELL_OFFLINE;
    let line = CODEX_SHELL_OFFLINE.trim();
    assert!(
        line.contains("curl") && line.contains("Web search"),
        "{line}"
    );
    let profile = profile();
    for macos in [false, true] {
        let codex = profile.codex_instructions_for(macos);
        assert_eq!(codex.matches(line).count(), 1, "{codex}");
        assert!(!profile.claude_instructions_for(macos).contains(line));
    }
}

/// Stage E's approval census: `curl` (a download, or a check that a link is alive) was Codex's
/// commonest repeated "run outside the sandbox" card on a Mac. So every Codex text, on macOS and
/// Windows, sends all web work (pages, judgments and statutes, downloads) to the browser tools,
/// names the download commands it must never use or ask to approve and why, and says what to do
/// with no browser. It covers research-rules' "open judgments and statutes" sentence too.
#[test]
fn codex_is_sent_to_the_browser_for_web_work_and_never_to_curl() {
    use clauseo_profile::instructions::CODEX_SHELL_OFFLINE;
    let line = CODEX_SHELL_OFFLINE.trim();
    let never = line
        .split("never use or ask to approve ")
        .nth(1)
        .expect(line);
    let never = never.split(". ").next().unwrap();
    for command in ["`curl`", "`wget`", "`iwr`", "`nc`", "Python"] {
        assert!(never.contains(command), "{command}: {line}");
    }
    assert!(
        never.ends_with("Clauseo declines a shell download and your turn ends"),
        "{line}"
    );
    assert!(
        line.contains(
            "Open pages, judgments and statutes and download files with your browser tools;"
        ) && line.contains("with no browser, tell the lawyer."),
        "{line}"
    );
    let profile = profile();
    for macos in [false, true] {
        let codex = profile.codex_instructions_for(macos);
        assert!(
            codex.contains("judgments and statutes and download files with your browser tools")
        );
        assert!(codex.contains("`curl`") && codex.contains("`wget`") && codex.contains("`iwr`"));
    }
}

/// R22: a page the fetch tool or the site refuses is never retried in disguise. Claude's line
/// comes right after the instructions and Codex's sits in its offline line, on every OS and
/// whatever the browser part, each in the reviewed words; neither agent gets the other's.
#[test]
fn each_agent_is_told_never_to_pose_as_a_browser_when_a_site_refuses() {
    use clauseo_profile::instructions::{CLAUDE_WEB, CODEX_SHELL_OFFLINE};
    let claude_line = "If the fetch tool or the site refuses a page, never retry with a browser \
                       user-agent or any other disguise; tell the lawyer which one refused and \
                       give the link to open.";
    let codex_line = "If a page cannot be opened or the site refuses, never retry by posing as a \
                      person's browser; use Clauseo's browser if available, otherwise tell the \
                      lawyer which refused and give the link.";
    assert_eq!(CLAUDE_WEB.trim(), claude_line);
    assert!(
        CODEX_SHELL_OFFLINE.contains(codex_line),
        "{CODEX_SHELL_OFFLINE}"
    );
    let profile = profile();
    for macos in [false, true] {
        let claude = profile.claude_instructions_for(macos);
        let codex = profile.codex_instructions_for(macos);
        assert_eq!(claude.matches(claude_line).count(), 1, "{claude}");
        assert_eq!(codex.matches(codex_line).count(), 1, "{codex}");
        assert!(!claude.contains(codex_line) && !codex.contains(claude_line));
    }
    assert!(!profile.instructions().contains("disguise"));
}
