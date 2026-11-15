//! The launch's say over Claude's subagents and its "Working at scale" part (`subagents.rs`),
//! through `session_meta` as a launch runs it, on the profile's own agents and part.

use std::io::Write;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};

use super::browser::Switch;
use super::profile::{ClaudeBrowser, ClaudeKeys, Folders, Login};
use super::subagents::helpers;
use super::{McpControl, session_meta};
use crate::browser::seed_sites::SEED_SITES;
use clauseo_profile::instructions::{SCALE_RESEARCH, claude_scale};
use clauseo_profile::subagents::{
    PAPER_READER, RESEARCHER, RESEARCHER_ARM, researcher_tools, subagents,
};

const ON: Switch = Switch {
    on: true,
    locked: false,
};
const OFF: Switch = Switch {
    on: false,
    locked: false,
};

static NO_DENY: crate::agent::data_fence::LaunchDeny = crate::agent::data_fence::LaunchDeny {
    spellings: Vec::new(),
    targets: Vec::new(),
    kept: Vec::new(),
    own_reads: Vec::new(),
};

fn keys(web_search: Switch, login: Login, browser: bool) -> ClaudeKeys<'static> {
    ClaudeKeys {
        web_search,
        login,
        folders: Folders {
            work: "/data/run/s/work",
            notes: "/data/agent-state/m/notes",
            connectors: "",
            browser: "",
        },
        seed_sites: SEED_SITES,
        forced_ask: false,
        data: &NO_DENY,
        case_law: false,
        browser: if browser {
            ClaudeBrowser::Shell
        } else {
            ClaudeBrowser::None
        },
        sandbox: None,
        matter: None,
        trusted_servers: &[],
    }
}

/// The profile's own append, as it ends: the shared text, then the part.
fn profile_append() -> String {
    format!("Legal text.\n\n{}", claude_scale(false).unwrap())
}

/// The profile's `_meta` for Claude, reduced to the append and `options.agents`.
fn profile_meta() -> Map<String, Value> {
    let meta = json!({
        "systemPrompt": {"append": profile_append()},
        "claudeCode": {"options": {"agents": subagents()}},
    });
    meta.as_object().unwrap().clone()
}

/// `session_meta` over [`profile_meta`], as a launch runs it: the agents left, and the append.
fn launched(keys: &ClaudeKeys<'_>) -> (Map<String, Value>, String) {
    let meta = session_meta(Some(profile_meta()), McpControl::Clauseo, None, Some(keys));
    let agents = meta["claudeCode"]["options"]
        .get("agents")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let append = meta["systemPrompt"]["append"].as_str().unwrap().to_owned();
    (agents, append)
}

/// The agents left after a launch with `keys`.
fn agents_after(keys: &ClaudeKeys<'_>) -> Map<String, Value> {
    launched(keys).0
}

fn tools(agent: &Value) -> Vec<&str> {
    agent["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool.as_str().unwrap())
        .collect()
}

/// What a tracing subscriber writes, for the log line.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The researcher's tools are the web's and the browser guard's, by exact name, with no `Bash`
/// form in any session that has it: browser on or off, web search on, Anthropic's login.
#[test]
fn the_researcher_never_gets_a_shell_rule() {
    assert!(RESEARCHER_ARM.is_some(), "the researcher ships");
    for browser in [false, true] {
        let agents = agents_after(&keys(ON, Login::FirstParty, browser));
        let researcher = tools(&agents[RESEARCHER]);
        assert_eq!(researcher, researcher_tools(), "browser {browser}");
        assert!(researcher.contains(&"mcp__browser__agent_browser_open"));
        for tool in researcher.iter().chain(&tools(&agents[PAPER_READER])) {
            assert!(
                !tool.starts_with("Bash") && !tool.starts_with("PowerShell"),
                "{tool}: browser {browser}"
            );
            assert!(!tool.contains("agent-browser"), "{tool}");
        }
    }
    // A cloud login has no researcher to give tools to.
    for login in [Login::Bedrock, Login::OtherCloud] {
        assert!(agents_after(&keys(ON, login, true)).is_empty(), "{login:?}");
    }
}

/// On Anthropic's own login the helpers keep Haiku and the append keeps "Working at scale".
/// On a cloud login, where Haiku may not be enabled, Claude gets no helpers and no part, so the
/// lawyer gets one agent, and the host's log says why.
#[test]
fn a_cloud_login_gets_no_helpers_and_no_scale_text() {
    let (first, append) = launched(&keys(ON, Login::FirstParty, false));
    let names: Vec<&str> = first.keys().map(String::as_str).collect();
    assert_eq!(names, [PAPER_READER, RESEARCHER]);
    assert_eq!(first[PAPER_READER]["model"], "haiku");
    assert_eq!(first[RESEARCHER]["model"], "haiku");
    assert_eq!(append, profile_append());
    assert!(append.contains("## Working at scale") && append.contains("paper-reader"));
    for login in [Login::Bedrock, Login::OtherCloud] {
        let captured = Captured::default();
        let sink = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || sink.clone())
            .finish();
        let meta = tracing::subscriber::with_default(subscriber, || {
            session_meta(
                Some(profile_meta()),
                McpControl::Clauseo,
                None,
                Some(&keys(ON, login, true)),
            )
        });
        assert!(
            meta["claudeCode"]["options"].get("agents").is_none(),
            "{login:?}"
        );
        assert_eq!(meta["systemPrompt"]["append"], "Legal text.", "{login:?}");
        let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert!(log.contains("Claude gets no helpers"), "{log}");
        assert!(log.contains(&format!("{login:?}")), "{log}");
    }
}

/// The research bullets go in only beside a researcher: with `SHIP_RESEARCH_TEXT` forced on,
/// web search off leaves neither, web search on both; a cloud login gets neither either way.
#[test]
fn the_research_text_comes_only_with_the_researcher() {
    let research = SCALE_RESEARCH.trim();
    for (login, web_search) in [
        (Login::FirstParty, false),
        (Login::FirstParty, true),
        (Login::Bedrock, true),
        (Login::OtherCloud, true),
    ] {
        let mut meta = profile_meta();
        let mut options = meta["claudeCode"]["options"].as_object().unwrap().clone();
        helpers(&mut meta, &mut options, login, web_search, true);
        let researcher = options
            .get("agents")
            .is_some_and(|agents| agents.get(RESEARCHER).is_some());
        let append = meta["systemPrompt"]["append"].as_str().unwrap();
        let both = login == Login::FirstParty && web_search;
        assert_eq!(researcher, both, "{login:?} web {web_search}");
        assert_eq!(
            append.contains(research),
            both,
            "{login:?} web {web_search}"
        );
        if both {
            let part = claude_scale(true).unwrap();
            assert_eq!(append, format!("Legal text.\n\n{part}"));
        }
    }
    // As shipped (`SHIP_RESEARCH_TEXT` off), no session gets the bullets.
    let (agents, append) = launched(&keys(ON, Login::FirstParty, true));
    assert!(agents.contains_key(RESEARCHER) && !append.contains(research));
}

/// Without web search the researcher would have no tool (the web's are denied, and Claude has no
/// browser server), and Claude Code refuses such a helper mid-answer: it is left out instead.
#[test]
fn without_web_search_the_researcher_is_left_out() {
    for browser in [false, true] {
        let agents = agents_after(&keys(OFF, Login::FirstParty, browser));
        let names: Vec<&str> = agents.keys().map(String::as_str).collect();
        assert_eq!(names, [PAPER_READER], "browser {browser}");
        assert_eq!(agents[PAPER_READER], subagents()[PAPER_READER]);
    }
    // A launch with no agents at all gets none.
    let meta = session_meta(
        None,
        McpControl::Clauseo,
        None,
        Some(&keys(OFF, Login::FirstParty, false)),
    );
    assert!(meta["claudeCode"]["options"].get("agents").is_none());
}
