//! The draft pickers on a fresh data folder (`actor/agent_options/seed.rs`): before a Claude or
//! Codex agent's first session, `ListAgents` gives its seed, the model and effort each at the
//! seed's one value, the agent's own default; a first conversation started from it sends no
//! `session/set_config_option` before its first prompt (`pickers-seed`, `pickers-seed-codex`
//! take the prompt right after the session's start, so a set there is a mismatch), so the agent
//! runs its default, and the pickers then move to what the agent reports. A pick the agent does
//! not offer is dropped and the default runs; a `SetConfigOption` of the seed's value is refused.
//! The dropped pick's log line: `pickers_seed_log.rs`.

use clauseo_protocol::{
    AgentId, CommandOutcome, ConfigOption, ConfigOptionKind, ConfigPicks, ErrorCode, HostCommand,
    HostQuery, MatterId, QueryResult,
};

use crate::codex::CODEX;
use crate::matters::summary;
use crate::pickers::{CLAUDE, current, logged, picks, set, turn};
use crate::support::{Dir, Running, Scenario};

/// The seed's one value (`seed.rs`'s `AGENT_DEFAULT`, `ui/app/src/room/pickers.ts`'s
/// `AGENT_DEFAULT_VALUE`).
pub(crate) const AGENT_DEFAULT: &str = "clauseo-agent-default";

/// The options `ListAgents` gives `agent`.
fn agent_options(host: &Running, agent: &str) -> Vec<ConfigOption> {
    let QueryResult::Agents(agents) = host.query(HostQuery::ListAgents).unwrap() else {
        panic!("ListAgents answered another result");
    };
    agents
        .into_iter()
        .find(|summary| summary.id.as_str() == agent)
        .map(|summary| summary.config_options)
        .unwrap_or_default()
}

/// Each select option's id, current value and values, in order.
fn shape(options: &[ConfigOption]) -> Vec<(String, String, Vec<String>)> {
    options
        .iter()
        .filter_map(|option| match &option.kind {
            ConfigOptionKind::Select {
                current_value,
                options,
            } => Some((
                option.id.to_string(),
                current_value.to_string(),
                options.iter().map(|c| c.value.to_string()).collect(),
            )),
            _ => None,
        })
        .collect()
}

/// The seed as `ListAgents` should give it: the model and the effort, ids as the agent's.
fn seeded(effort: &str) -> Vec<(String, String, Vec<String>)> {
    ["model", effort]
        .into_iter()
        .map(|id| {
            let value = AGENT_DEFAULT.to_owned();
            (id.to_owned(), value.clone(), vec![value])
        })
        .collect()
}

/// A fresh data folder's first conversation with `agent`, its draft's config `config`: the
/// seed shows before it, the prompt goes with nothing set, and the thread's and the kept
/// options are then the agent's `model` and `effort` values.
fn first_conversation(
    test: &str,
    agent: &str,
    scenario: &'static str,
    effort_id: &str,
    config: Option<ConfigPicks>,
    ran: (&str, &str),
) -> (Dir, Running, MatterId) {
    let dir = Dir::new(test, &[(agent, Scenario::Bundled(scenario))]);
    let host = dir.start(0);
    assert_eq!(shape(&agent_options(&host, agent)), seeded(effort_id));
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let outcome = host.ok(HostCommand::CreateThread {
        matter,
        agent: AgentId::new(agent).unwrap(),
        config,
    });
    let CommandOutcome::ThreadCreated { thread } = outcome else {
        panic!("CreateThread answered {outcome:?}");
    };
    // `turn` checks the fake agent saw no mismatch: no set before the prompt.
    let events = turn(&host, thread, "Summarise clause 4.");
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some(ran.0));
    assert_eq!(current(&options, effort_id).as_deref(), Some(ran.1));
    let kept = agent_options(&host, agent);
    assert_eq!(current(&kept, "model").as_deref(), Some(ran.0));
    assert_eq!(current(&kept, effort_id).as_deref(), Some(ran.1));
    (dir, host, matter)
}

/// Claude on a fresh data folder: the seed shows, the first prompt goes with no model or effort
/// set (the agent's default, Opus at medium in `pickers-seed`), and the pickers move to it.
#[test]
fn a_fresh_claude_shows_its_default_and_runs_it() {
    let (_dir, host, matter) = first_conversation(
        "pickers_seed_claude",
        CLAUDE,
        "pickers-seed",
        "effort",
        None,
        ("opus", "medium"),
    );
    assert_eq!(summary(&host, matter).agent_prefs, None);
    host.shutdown();
}

/// Codex on a fresh data folder: the same, in Codex's ids (GPT-6 at medium).
#[test]
fn a_fresh_chatgpt_shows_its_default_and_runs_it() {
    let (_dir, host, matter) = first_conversation(
        "pickers_seed_codex",
        CODEX,
        "pickers-seed-codex",
        "reasoning_effort",
        None,
        ("gpt-6", "medium"),
    );
    assert_eq!(summary(&host, matter).agent_prefs, None);
    host.shutdown();
}

/// A draft that sends the seed's own values as its config: neither reaches the agent, on both
/// agents, and nothing is dropped from anywhere since nothing was asked of the agent.
#[test]
fn the_seeds_default_sent_as_config_is_never_set() {
    for (test, agent, scenario, effort, ran) in [
        (
            "pickers_seed_sent_claude",
            CLAUDE,
            "pickers-seed",
            "effort",
            ("opus", "medium"),
        ),
        (
            "pickers_seed_sent_codex",
            CODEX,
            "pickers-seed-codex",
            "reasoning_effort",
            ("gpt-6", "medium"),
        ),
    ] {
        let config = picks(&[("model", AGENT_DEFAULT), (effort, AGENT_DEFAULT)]);
        let (_dir, host, _) = first_conversation(test, agent, scenario, effort, Some(config), ran);
        host.shutdown();
    }
}

/// A pick the agent does not offer (a model gone from the account) goes the stale way: never
/// sent, the prompt runs on the agent's default, the pickers show the model in use, and nothing
/// errors. A `SetConfigOption` of the seed's value is refused, before and after the session.
#[test]
fn an_unknown_pick_is_dropped_and_the_default_runs() {
    let dir = Dir::new(
        "pickers_seed_unknown",
        &[(CLAUDE, Scenario::Bundled("pickers-seed"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let config = picks(&[("model", "claude-gone-9"), ("effort", AGENT_DEFAULT)]);
    let outcome = host.ok(HostCommand::CreateThread {
        matter,
        agent: AgentId::new(CLAUDE).unwrap(),
        config: Some(config),
    });
    let CommandOutcome::ThreadCreated { thread } = outcome else {
        panic!("CreateThread answered {outcome:?}");
    };
    host.refused(
        set(thread, "model", AGENT_DEFAULT),
        ErrorCode::CommandInvalid,
    );
    let events = turn(&host, thread, "Summarise clause 4.");
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some("opus"));
    assert_eq!(current(&options, "effort").as_deref(), Some("medium"));
    assert_eq!(
        current(&agent_options(&host, CLAUDE), "model").as_deref(),
        Some("opus")
    );
    host.refused(
        set(thread, "effort", AGENT_DEFAULT),
        ErrorCode::CommandInvalid,
    );
    assert_eq!(summary(&host, matter).agent_prefs, None);
    host.shutdown();
}
