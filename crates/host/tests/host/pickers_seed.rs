//! The draft pickers on a fresh data folder with no model probe (the harness's reader probes
//! nothing): there is no seed, so before a Claude or Codex agent's first session `ListAgents`
//! gives it no options and no models; a first conversation then sends no
//! `session/set_config_option` before its first prompt (`pickers-seed`, `pickers-seed-codex`
//! take the prompt right after the session's start, so a set there is a mismatch), so the agent
//! runs its default, and the pickers then show what the agent reports. A pick the agent does
//! not offer is dropped and the default runs. The dropped pick's log line:
//! `pickers_seed_log.rs`. With a probe: `pickers_probe.rs`.

use clauseo_protocol::{
    AgentId, AgentSummary, CommandOutcome, ConfigOption, ConfigPicks, HostCommand, HostQuery,
    MatterId, QueryResult,
};

use crate::codex::CODEX;
use crate::matters::summary;
use crate::pickers::{CLAUDE, current, logged, picks, turn};
use crate::support::{Dir, Running, Scenario};

/// What `ListAgents` gives `agent`.
pub(crate) fn agent_summary(host: &Running, agent: &str) -> AgentSummary {
    let QueryResult::Agents(agents) = host.query(HostQuery::ListAgents).unwrap() else {
        panic!("ListAgents answered another result");
    };
    agents
        .into_iter()
        .find(|summary| summary.id.as_str() == agent)
        .expect("the agent is listed")
}

/// The options `ListAgents` gives `agent`.
fn agent_options(host: &Running, agent: &str) -> Vec<ConfigOption> {
    agent_summary(host, agent).config_options
}

/// A fresh data folder's first conversation with `agent`, its draft's config `config`: no
/// options or models show before it, the prompt goes with nothing set, and the thread's and the
/// kept options are then the agent's `model` and `effort` values.
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
    let fresh = agent_summary(&host, agent);
    assert!(fresh.config_options.is_empty(), "no seed");
    assert_eq!(fresh.models, None);
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

/// Claude on a fresh data folder: nothing shows, the first prompt goes with no model or effort
/// set (the agent's default, Opus at medium in `pickers-seed`), and the pickers then show it.
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

/// A pick the agent does not offer (a model gone from the account) goes the stale way: never
/// sent, the prompt runs on the agent's default, the pickers show the model in use, and nothing
/// errors.
#[test]
fn an_unknown_pick_is_dropped_and_the_default_runs() {
    let dir = Dir::new(
        "pickers_seed_unknown",
        &[(CLAUDE, Scenario::Bundled("pickers-seed"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let config = picks(&[("model", "claude-gone-9")]);
    let outcome = host.ok(HostCommand::CreateThread {
        matter,
        agent: AgentId::new(CLAUDE).unwrap(),
        config: Some(config),
    });
    let CommandOutcome::ThreadCreated { thread } = outcome else {
        panic!("CreateThread answered {outcome:?}");
    };
    let events = turn(&host, thread, "Summarise clause 4.");
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some("opus"));
    assert_eq!(current(&options, "effort").as_deref(), Some("medium"));
    assert_eq!(
        current(&agent_options(&host, CLAUDE), "model").as_deref(),
        Some("opus")
    );
    let kept = agent_summary(&host, CLAUDE)
        .models
        .expect("the session's models");
    assert_eq!(kept.current_model, "opus");
    assert_eq!(kept.model("opus").unwrap().name, "Opus 5.5");
    assert_eq!(summary(&host, matter).agent_prefs, None);
    host.shutdown();
}
