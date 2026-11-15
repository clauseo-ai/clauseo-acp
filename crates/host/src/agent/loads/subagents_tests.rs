//! The launch's say over Claude's subagents (`subagents.rs`), through `claude_options` as a
//! launch runs it, on the profile's own agents.

use std::io::Write;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};

use super::browser::Switch;
use super::profile::{ClaudeKeys, Folders, Login, claude_options};
use super::subagents::INHERIT;
use crate::browser::seed_sites::SEED_SITES;
use clauseo_profile::subagents::{
    PAPER_READER, PAPER_READER_MODEL, RESEARCHER, RESEARCHER_ARM, researcher_tools, subagents,
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
        browser,
        sandbox: None,
        matter: None,
        trusted_servers: &[],
    }
}

/// `claude_options` over the profile's `options.agents`, as a launch merges it.
fn agents_after(keys: &ClaudeKeys<'_>) -> Map<String, Value> {
    let mut options = Map::new();
    options.insert("agents".to_owned(), Value::Object(subagents()));
    claude_options(&mut options, keys);
    options
        .get("agents")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
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
/// form in any session: browser on or off, web search on, any login.
#[test]
fn the_researcher_never_gets_a_shell_rule() {
    assert!(RESEARCHER_ARM.is_some(), "the researcher ships");
    for login in [Login::FirstParty, Login::Bedrock, Login::OtherCloud] {
        for browser in [false, true] {
            let agents = agents_after(&keys(ON, login, browser));
            let researcher = tools(&agents[RESEARCHER]);
            assert_eq!(
                researcher,
                researcher_tools(),
                "{login:?} browser {browser}"
            );
            assert!(researcher.contains(&"mcp__browser__agent_browser_open"));
            for tool in researcher.iter().chain(&tools(&agents[PAPER_READER])) {
                assert!(
                    !tool.starts_with("Bash") && !tool.starts_with("PowerShell"),
                    "{tool}: {login:?} browser {browser}"
                );
                assert!(!tool.contains("agent-browser"), "{tool}");
            }
        }
    }
}

/// On Anthropic's own login each helper keeps the measured model; on a cloud login, where Haiku
/// may not be enabled, each runs on the conversation's model, and the host says so in its log.
#[test]
fn a_cloud_login_runs_the_helpers_on_the_conversations_model() {
    let first = agents_after(&keys(ON, Login::FirstParty, false));
    assert_eq!(first[PAPER_READER]["model"], PAPER_READER_MODEL);
    let arm = RESEARCHER_ARM.unwrap();
    assert_eq!(first[RESEARCHER]["model"], arm.model());
    for login in [Login::Bedrock, Login::OtherCloud] {
        let captured = Captured::default();
        let sink = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || sink.clone())
            .finish();
        let agents =
            tracing::subscriber::with_default(subscriber, || agents_after(&keys(ON, login, false)));
        for (name, agent) in &agents {
            assert_eq!(agent["model"], INHERIT, "{name} on {login:?}");
            // Only the model changes: effort, turns, prompt and tools stay the profile's.
            let mut profile = subagents()[name].clone();
            profile["model"] = json!(INHERIT);
            if name == RESEARCHER {
                profile["tools"] = json!(researcher_tools());
            }
            assert_eq!(agent, &profile, "{name}");
        }
        let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert!(
            log.contains("helpers run on the conversation's model"),
            "{log}"
        );
        assert!(log.contains(&format!("{login:?}")), "{log}");
    }
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
    let mut options = Map::new();
    claude_options(&mut options, &keys(OFF, Login::Bedrock, false));
    assert!(!options.contains_key("agents"));
}
