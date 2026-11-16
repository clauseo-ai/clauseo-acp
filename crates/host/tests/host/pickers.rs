//! The model and effort pickers (`actor/turn/picks.rs`, `actor/agent_options.rs`), with a fake
//! agent named `claude-acp` reporting Claude's options (`pickers-*.jsonl`): `ListAgents` keeps an
//! agent's options from its first conversation, across a restart too; `CreateThread::config`
//! and a matter's prefs are set before the first prompt; a `SetConfigOption` of the model, then
//! the effort, reaches the agent and is kept in the matter's prefs; only model and effort
//! options are settable, before the first session too; a pick the agent does not take is
//! dropped and the prompt still runs, while one the agent fails otherwise, or one of an option
//! the session does not report, is kept; a pick of a mode is never sent and is dropped; a
//! reloaded session gets the thread's own values back; options with real values and no Default
//! rows (the AIR `recommendedValue` capability) need nothing more.

use std::collections::BTreeMap;

use clauseo_protocol::{
    AgentChoiceId, AgentId, AgentKind, AgentPrefs, CommandOutcome, ConfigOption, ConfigOptionKind,
    ConfigPicks, ConfigValue, DomainEvent, ErrorCode, FieldChange, HostCommand, HostQuery,
    MatterId, QueryResult, StopReason, StreamId, ThreadId,
};

use crate::matters::summary;
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

/// The agent id that makes a fake agent a Claude agent, so picks are kept per kind.
const CLAUDE: &str = "claude-acp";

fn id(text: &str) -> AgentChoiceId {
    AgentChoiceId::new(text).unwrap()
}

fn picks(pairs: &[(&str, &str)]) -> ConfigPicks {
    ConfigPicks::new(pairs.iter().map(|(o, v)| (id(o), id(v))).collect()).unwrap()
}

fn claude_prefs(pairs: &[(&str, &str)]) -> AgentPrefs {
    AgentPrefs(BTreeMap::from([(AgentKind::Claude, picks(pairs))]))
}

fn set(thread: ThreadId, option: &str, value: &str) -> HostCommand {
    HostCommand::SetConfigOption {
        thread,
        option_id: id(option),
        value: ConfigValue::Select(id(value)),
    }
}

/// The current value of the select option `option` in `options`.
fn current(options: &[ConfigOption], option: &str) -> Option<String> {
    options
        .iter()
        .find(|offered| offered.id == option)
        .and_then(|offered| match &offered.kind {
            ConfigOptionKind::Select { current_value, .. } => Some(current_value.to_string()),
            _ => None,
        })
}

/// The values of the select option `option` in `options`.
fn values(options: &[ConfigOption], option: &str) -> Vec<String> {
    options
        .iter()
        .find(|offered| offered.id == option)
        .map(|offered| match &offered.kind {
            ConfigOptionKind::Select { options, .. } => options
                .iter()
                .map(|choice| choice.value.to_string())
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

/// Every logged `ConfigOptionsReplaced`, in order.
fn replaced(events: &Events) -> Vec<Vec<ConfigOption>> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ConfigOptionsReplaced(replaced) => Some(replaced.options.clone()),
            _ => None,
        })
        .collect()
}

/// The thread's options as last logged.
fn logged(events: &Events) -> Vec<ConfigOption> {
    replaced(events).pop().unwrap_or_default()
}

/// The options `ListAgents` gives `claude-acp`.
fn agent_options(host: &Running) -> Vec<ConfigOption> {
    let QueryResult::Agents(agents) = host.query(HostQuery::ListAgents).unwrap() else {
        panic!("ListAgents answered another result");
    };
    agents
        .into_iter()
        .find(|agent| agent.id.as_str() == CLAUDE)
        .map(|agent| agent.config_options)
        .unwrap_or_default()
}

fn create_thread(host: &Running, matter: MatterId, config: Option<ConfigPicks>) -> ThreadId {
    let outcome = host.ok(HostCommand::CreateThread {
        matter,
        agent: AgentId::new(CLAUDE).unwrap(),
        config,
    });
    let CommandOutcome::ThreadCreated { thread } = outcome else {
        panic!("CreateThread answered {outcome:?}");
    };
    thread
}

fn set_prefs(host: &Running, matter: MatterId, prefs: FieldChange<AgentPrefs>) {
    host.ok(HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        skill_packs: FieldChange::Keep,
        pinned: None,
        agent_prefs: prefs,
    });
}

/// Runs one prompt on `thread` to its end and returns the thread's events so far.
fn turn(host: &Running, thread: ThreadId, text: &str) -> Events {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, text);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    events
}

/// (a) the agent's options after its first conversation and after a restart, (f) what may not
/// be set, (c) the model then the effort set mid-conversation and kept in the matter's prefs, and
/// (d) a new thread in the matter starting with them.
#[test]
fn a_first_conversation_keeps_the_options_and_a_pick_becomes_the_matters() {
    let dir = Dir::new(
        "pickers_first",
        &[(CLAUDE, Scenario::Bundled("pickers-first"))],
    );
    let host = dir.start(0);
    assert!(agent_options(&host).is_empty(), "none before any session");
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    // Nothing tells the option's category yet: the command starts the session, whose options
    // then refuse it before it reaches the agent (`pickers-first` takes a prompt next).
    host.refused(set(thread, "mode", "plan"), ErrorCode::CommandInvalid);
    let mut events = turn(&host, thread, "Summarise clause 4.");
    let kept = agent_options(&host);
    assert_eq!(current(&kept, "model").as_deref(), Some("default"));
    assert_eq!(values(&kept, "effort"), ["default", "low", "high", "max"]);

    // Only a choice of the model or the effort; none of these reaches the agent.
    let boolean = |option: &str| HostCommand::SetConfigOption {
        thread,
        option_id: id(option),
        value: ConfigValue::Boolean(true),
    };
    for refused in [
        set(thread, "mode", "plan"),
        set(thread, "fast", "on"),
        boolean("fast"),
        boolean("model"),
        set(thread, "nope", "x"),
    ] {
        host.refused(refused, ErrorCode::CommandInvalid);
    }

    assert_eq!(host.ok(set(thread, "model", "opus")), CommandOutcome::Done);
    events.until("model opus", |e| {
        current(&logged(e), "model").as_deref() == Some("opus")
    });
    assert_eq!(
        summary(&host, matter).agent_prefs,
        Some(claude_prefs(&[("model", "opus")]))
    );
    assert_eq!(host.ok(set(thread, "effort", "high")), CommandOutcome::Done);
    events.until("effort high", |e| {
        current(&logged(e), "effort").as_deref() == Some("high")
    });
    events.assert_no_mismatch();
    assert_eq!(
        summary(&host, matter).agent_prefs,
        Some(claude_prefs(&[("model", "opus"), ("effort", "high")]))
    );
    assert_eq!(
        current(&agent_options(&host), "effort").as_deref(),
        Some("high")
    );
    host.shutdown();

    // The kept options outlive the host; a new thread in the matter starts with its prefs,
    // set before its first prompt (`pickers-picked` refuses a prompt before them).
    dir.set_agents(&[(CLAUDE, Scenario::Bundled("pickers-picked"))]);
    let host = dir.start(1);
    let kept = agent_options(&host);
    assert_eq!(current(&kept, "model").as_deref(), Some("opus"));
    assert_eq!(current(&kept, "effort").as_deref(), Some("high"));
    let second = create_thread(&host, matter, None);
    let events = turn(&host, second, "And clause 5?");
    // One logged list, the start's with the picks set: the update the agent sent while they were
    // set is older than its last answer, and is dropped.
    let lists = replaced(&events);
    assert_eq!(lists.len(), 1, "{}", events.dump());
    assert_eq!(current(&lists[0], "model").as_deref(), Some("opus"));
    assert_eq!(current(&lists[0], "effort").as_deref(), Some("high"));
    host.shutdown();
}

/// (b) `CreateThread::config` is set before the first prompt and leaves the matter's prefs as
/// they were; an empty config sets nothing, even with prefs.
#[test]
fn create_thread_config_is_set_before_the_first_prompt() {
    let dir = Dir::new(
        "pickers_config",
        &[(CLAUDE, Scenario::Bundled("pickers-picked"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let config = picks(&[("model", "opus"), ("effort", "high")]);
    let thread = create_thread(&host, matter, Some(config));
    let events = turn(&host, thread, "Summarise clause 4.");
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some("opus"));
    assert_eq!(current(&options, "effort").as_deref(), Some("high"));
    assert_eq!(summary(&host, matter).agent_prefs, None);
    set_prefs(
        &host,
        matter,
        FieldChange::Set(claude_prefs(&[("model", "haiku")])),
    );
    host.shutdown();

    // `pickers-first` takes the prompt right after `session/new`: a pick sent is a mismatch.
    dir.set_agents(&[(CLAUDE, Scenario::Bundled("pickers-first"))]);
    let host = dir.start(1);
    let none = create_thread(&host, matter, Some(ConfigPicks::default()));
    let events = turn(&host, none, "Summarise clause 5.");
    assert_eq!(
        current(&logged(&events), "model").as_deref(),
        Some("default")
    );
    host.shutdown();
}

/// (e) A pick the agent refuses (the model) and one it no longer offers (the effort) are
/// dropped: the prompt runs at the agent's values, the matter's prefs lose both, and the kept
/// options are the live ones.
#[test]
fn a_stale_pick_is_dropped_and_the_question_still_runs() {
    let dir = Dir::new(
        "pickers_stale",
        &[(CLAUDE, Scenario::Bundled("pickers-stale"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let stale = claude_prefs(&[("model", "opus"), ("effort", "max")]);
    set_prefs(&host, matter, FieldChange::Set(stale));
    let thread = create_thread(&host, matter, None);
    let events = turn(&host, thread, "Summarise clause 4.");
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some("default"));
    assert_eq!(current(&options, "effort").as_deref(), Some("default"));
    assert_eq!(summary(&host, matter).agent_prefs, None);
    assert_eq!(
        values(&agent_options(&host), "effort"),
        ["default", "low", "high"]
    );
    host.shutdown();
}

/// (g) A session loaded at the agent's defaults gets the thread's own model and effort back
/// before the prompt.
#[test]
fn a_loaded_session_gets_the_threads_values_back() {
    let dir = Dir::new(
        "pickers_load",
        &[(CLAUDE, Scenario::Bundled("pickers-picked"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let config = picks(&[("model", "opus"), ("effort", "high")]);
    let thread = create_thread(&host, matter, Some(config));
    drop(turn(&host, thread, "Summarise clause 4."));
    host.shutdown();

    dir.set_agents(&[(CLAUDE, Scenario::Bundled("pickers-load"))]);
    let host = dir.start(1);
    let events = turn(&host, thread, "And clause 5?");
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some("opus"));
    assert_eq!(current(&options, "effort").as_deref(), Some("high"));
    host.shutdown();
}

/// (h) A pick the agent fails with an error other than -32602 (a rate limit) is kept, and a
/// stored pick of a mode, which the pickers never set, is never sent and is dropped
/// (`pickers-busy` offers `bypassPermissions`; a mode pick sent is a mismatch).
#[test]
fn a_failed_pick_is_kept_and_a_mode_pick_is_never_sent() {
    let dir = Dir::new(
        "pickers_busy",
        &[(CLAUDE, Scenario::Bundled("pickers-busy"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let stored = claude_prefs(&[("model", "opus"), ("mode", "bypassPermissions")]);
    set_prefs(&host, matter, FieldChange::Set(stored));
    let thread = create_thread(&host, matter, None);
    let events = turn(&host, thread, "Summarise clause 4.");
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some("default"));
    assert_eq!(current(&options, "mode").as_deref(), Some("auto"));
    assert_eq!(
        summary(&host, matter).agent_prefs,
        Some(claude_prefs(&[("model", "opus")]))
    );
    host.shutdown();
}

/// (i) A session that reports no options leaves the picks in the matter's prefs, and the next
/// session that reports them gets them set before its prompt.
#[test]
fn a_pick_waits_for_a_session_with_its_option() {
    let dir = Dir::new(
        "pickers_bare",
        &[(CLAUDE, Scenario::Bundled("pickers-bare"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let prefs = claude_prefs(&[("model", "opus"), ("effort", "high")]);
    set_prefs(&host, matter, FieldChange::Set(prefs.clone()));
    let first = create_thread(&host, matter, None);
    drop(turn(&host, first, "Summarise clause 4."));
    assert_eq!(summary(&host, matter).agent_prefs, Some(prefs.clone()));
    host.shutdown();

    dir.set_agents(&[(CLAUDE, Scenario::Bundled("pickers-picked"))]);
    let host = dir.start(1);
    let second = create_thread(&host, matter, None);
    let options = logged(&turn(&host, second, "And clause 5?"));
    assert_eq!(current(&options, "model").as_deref(), Some("opus"));
    assert_eq!(current(&options, "effort").as_deref(), Some("high"));
    assert_eq!(summary(&host, matter).agent_prefs, Some(prefs));
    host.shutdown();
}

/// (j) Under the AIR `recommendedValue` capability Claude reports real values and no Default
/// rows (`pickers-recommended`, from claude-agent-acp 0.89.1 source): the kept options and the
/// thread's are "opus" and "medium", a stored pick of `default` is not offered so it is never
/// sent and leaves the matter's prefs, and a model change brings the effort back at "medium".
#[test]
fn recommended_values_need_no_default_row() {
    let dir = Dir::new(
        "pickers_recommended",
        &[(CLAUDE, Scenario::Bundled("pickers-recommended"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let legacy = claude_prefs(&[("model", "default"), ("effort", "default")]);
    set_prefs(&host, matter, FieldChange::Set(legacy));
    let thread = create_thread(&host, matter, None);
    let mut events = turn(&host, thread, "Summarise clause 4.");
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some("opus"));
    assert_eq!(current(&options, "effort").as_deref(), Some("medium"));
    assert_eq!(summary(&host, matter).agent_prefs, None);
    let kept = agent_options(&host);
    assert_eq!(values(&kept, "model"), ["opus", "haiku"]);
    assert_eq!(values(&kept, "effort"), ["low", "medium", "high", "max"]);
    assert_eq!(current(&kept, "effort").as_deref(), Some("medium"));

    assert_eq!(host.ok(set(thread, "model", "haiku")), CommandOutcome::Done);
    events.until("model haiku", |e| {
        current(&logged(e), "model").as_deref() == Some("haiku")
    });
    assert_eq!(
        current(&logged(&events), "effort").as_deref(),
        Some("medium")
    );
    events.assert_no_mismatch();
    assert_eq!(
        summary(&host, matter).agent_prefs,
        Some(claude_prefs(&[("model", "haiku")]))
    );
    host.shutdown();
}
