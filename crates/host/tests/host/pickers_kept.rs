//! A thread's kept model and effort (`actor/turn/picks/own.rs`) through fallback model lists,
//! with the stand-in agent as Codex: a relaunch whose list lacks the kept model runs on what it
//! offers with one notice, and a second turn of that session gets none; the matter's prefs keep
//! the model; after a host restart, a list that has it again gets it set back with no notice
//! (`pickers-kept-fallback`, then `pickers-kept-restored`). A model the lawyer picks on the
//! fallback list becomes the kept one (`pickers-kept-fallback-pick`, then
//! `pickers-kept-picked-restored`). A `CreateThread` config the first session holds or sets, and
//! a prompt's own pick the session already holds, are kept too (`pickers-kept-created`,
//! `pickers-kept-own-pick`).

use std::collections::BTreeMap;

use clauseo_protocol::{
    AgentId, AgentKind, AgentPrefs, CommandOutcome, DeliveryRequest, FieldChange, HostCommand,
    MatterId, NoticeKind, StopReason, StreamId, ThreadId, TurnId,
};

use crate::codex::CODEX;
use crate::idle_stop::{LIMIT, idle_stopped};
use crate::matters::summary;
use crate::observe::Events;
use crate::pickers::{picks, set};
use crate::pickers_idle::started_and_notices;
use crate::support::{Dir, Running, Scenario, text};

pub(crate) const LUNA_LOW: [(&str, &str); 2] =
    [("model", "gpt-6-luna"), ("reasoning_effort", "low")];

fn codex_prefs() -> AgentPrefs {
    AgentPrefs(BTreeMap::from([(AgentKind::Codex, picks(&LUNA_LOW))]))
}

pub(crate) fn names(model: &str, effort: &str) -> (Option<String>, Option<String>) {
    (Some(model.to_owned()), Some(effort.to_owned()))
}

/// The one notice a fallback session gives, naming what it ran.
fn told(model: &str) -> NoticeKind {
    NoticeKind::PickNotUsed {
        model: Some(model.to_owned()),
        effort: Some("Low".to_owned()),
    }
}

/// Runs `text` on `thread` to its end, with no mismatch against the scenario.
pub(crate) fn run(host: &Running, events: &mut Events, thread: ThreadId, text: &str) -> TurnId {
    let (turn, _) = host.prompt(thread, text);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    turn
}

/// [`run`] with `config` as the prompt's own picks.
fn run_with(
    host: &Running,
    events: &mut Events,
    thread: ThreadId,
    prompt: &str,
    config: &[(&str, &str)],
) -> TurnId {
    let outcome = host.ok(HostCommand::SendPrompt {
        thread,
        content: text(prompt),
        delivery: DeliveryRequest::Queue,
        config: Some(picks(config)),
        agent: None,
    });
    let CommandOutcome::PromptAccepted { turn, .. } = outcome else {
        panic!("SendPrompt answered {outcome:?}");
    };
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    turn
}

/// A Codex thread of `matter` (`config` its `CreateThread` config).
pub(crate) fn created(
    host: &Running,
    matter: MatterId,
    config: Option<&[(&str, &str)]>,
) -> ThreadId {
    let outcome = host.ok(HostCommand::CreateThread {
        matter,
        agent: AgentId::new(CODEX).unwrap(),
        config: config.map(picks),
    });
    let CommandOutcome::ThreadCreated { thread } = outcome else {
        panic!("CreateThread answered {outcome:?}");
    };
    thread
}

/// `thread`'s session stops idle: the idle limit passes with no prompt.
pub(crate) fn stop_idle(host: &Running, events: &mut Events) {
    host.wait_armed(LIMIT);
    // That may be the deadline an earlier turn's end armed: the clock answers every arming in
    // order. The `TurnEnded` read last is published before the host arms its own deadline, so
    // the round trip lets the host finish that input, and arm it, before the clock moves.
    host.settle();
    host.clock.advance(LIMIT);
    events.until("the idle stop", |events| {
        events.position(idle_stopped).is_some()
    });
    host.settle();
}

/// A Codex thread (`config` its `CreateThread` config) that ran one prompt and stopped idle.
pub(crate) fn idle_thread(
    host: &Running,
    matter: MatterId,
    config: Option<&[(&str, &str)]>,
) -> ThreadId {
    let thread = created(host, matter, config);
    let mut events = host.events(StreamId::Thread(thread));
    run(host, &mut events, thread, "Summarise clause 4.");
    stop_idle(host, &mut events);
    thread
}

/// The thread's matter's prefs hold gpt-6-luna and low; the thread has no config of its own,
/// so its first session keeps them (`pickers_kept_rows.rs` has a row from before them).
/// The fallback relaunch drops neither from the prefs, tells once, and the third turn nothing;
/// after a restart the list has gpt-6-luna again, and it is set back with no notice.
#[test]
fn a_fallback_relaunch_tells_once_keeps_the_values_and_a_restored_list_gets_them_back() {
    let dir = Dir::new(
        "pickers_kept_fallback",
        &[(CODEX, Scenario::Local("pickers-kept-fallback"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    host.ok(HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        skill_packs: FieldChange::Keep,
        pinned: None,
        agent_prefs: FieldChange::Set(codex_prefs()),
    });
    let thread = idle_thread(&host, matter, None);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let second = run(&host, &mut events, thread, "And clause 5?");
    let third = run(&host, &mut events, thread, "And clause 6?");
    let (started, notices) = started_and_notices(&events, second);
    assert_eq!(started, names("GPT-6", "Low"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6")], "{}", events.dump());
    assert_eq!(started_and_notices(&events, third).0, names("GPT-6", "Low"));
    assert_eq!(summary(&host, matter).agent_prefs, Some(codex_prefs()));
    host.shutdown();

    dir.set_agents(&[(CODEX, Scenario::Local("pickers-kept-restored"))]);
    let host = dir.start_with(1, |config| config.with_live_agents(2));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let fourth = run(&host, &mut events, thread, "And clause 7?");
    let (started, notices) = started_and_notices(&events, fourth);
    assert_eq!(started, names("GPT-6 Luna", "Low"), "{}", events.dump());
    // Only the fallback session's notice.
    assert_eq!(notices, [told("GPT-6")], "{}", events.dump());
    assert_eq!(summary(&host, matter).agent_prefs, Some(codex_prefs()));
    host.shutdown();
}

/// On the fallback list the lawyer picks gpt-6-astra: it becomes the thread's kept model, so
/// the next session, whose list has gpt-6-luna again, gets gpt-6-astra.
#[test]
fn a_model_picked_on_a_fallback_list_becomes_the_kept_one() {
    let dir = Dir::new(
        "pickers_kept_pick",
        &[(CODEX, Scenario::Local("pickers-kept-fallback-pick"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = idle_thread(&host, matter, Some(&LUNA_LOW));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let second = run(&host, &mut events, thread, "And clause 5?");
    assert_eq!(started_and_notices(&events, second).1, [told("GPT-6")]);
    host.ok(set(thread, "model", "gpt-6-astra"));
    host.shutdown();

    dir.set_agents(&[(CODEX, Scenario::Local("pickers-kept-picked-restored"))]);
    let host = dir.start_with(1, |config| config.with_live_agents(2));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let third = run(&host, &mut events, thread, "And clause 6?");
    let (started, notices) = started_and_notices(&events, third);
    assert_eq!(started, names("GPT-6 Astra", "Low"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6")], "{}", events.dump());
    host.shutdown();
}

/// A `CreateThread` config the first session holds (model) or sets (effort) is the thread's kept
/// values: after the agent's own update reports gpt-6 and medium, the relaunch after the idle
/// stop sets gpt-6-luna and low back, with no notice (`pickers-kept-created`).
#[test]
fn a_create_thread_config_is_kept_for_the_relaunch() {
    let dir = Dir::new(
        "pickers_kept_created",
        &[(CODEX, Scenario::Local("pickers-kept-created"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = idle_thread(&host, matter, Some(&LUNA_LOW));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let second = run(&host, &mut events, thread, "And clause 5?");
    let (started, notices) = started_and_notices(&events, second);
    assert_eq!(started, names("GPT-6 Luna", "Low"), "{}", events.dump());
    assert_eq!(notices, [], "{}", events.dump());
    host.shutdown();
}

/// A prompt's own pick the session already holds (no set goes out) is the thread's kept value
/// too. The agent's update reports gpt-6-astra; the second turn picks it; after the idle stop the
/// relaunch sets gpt-6-astra and low back, not the older kept gpt-6-luna, with no notice
/// (`pickers-kept-own-pick`).
#[test]
fn a_prompts_own_pick_the_session_holds_is_kept_over_the_older_one() {
    let dir = Dir::new(
        "pickers_kept_own_pick",
        &[(CODEX, Scenario::Local("pickers-kept-own-pick"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = created(&host, matter, Some(&LUNA_LOW));
    let mut events = host.events(StreamId::Thread(thread));
    run(&host, &mut events, thread, "Summarise clause 4.");
    let second = run_with(
        &host,
        &mut events,
        thread,
        "And clause 5?",
        &[("model", "gpt-6-astra")],
    );
    let (started, notices) = started_and_notices(&events, second);
    assert_eq!(started, names("GPT-6 Astra", "Low"), "{}", events.dump());
    assert_eq!(notices, [], "{}", events.dump());
    stop_idle(&host, &mut events);

    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let third = run(&host, &mut events, thread, "And clause 6?");
    let (started, notices) = started_and_notices(&events, third);
    assert_eq!(started, names("GPT-6 Astra", "Low"), "{}", events.dump());
    assert_eq!(notices, [], "{}", events.dump());
    host.shutdown();
}

/// A new thread with no picks keeps what its first session ran with at its first answer
/// (gpt-6-sol, the header then), not what the agent switched to later: the relaunch, whose list
/// lacks gpt-6-sol, says so once (`pickers-kept-first-header`).
#[test]
fn a_new_threads_first_session_seeds_its_kept_values() {
    let dir = Dir::new(
        "pickers_kept_first_header",
        &[(CODEX, Scenario::Local("pickers-kept-first-header"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = idle_thread(&host, matter, None);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let second = run(&host, &mut events, thread, "And clause 5?");
    let (started, notices) = started_and_notices(&events, second);
    assert_eq!(started, names("GPT-6", "Medium"), "{}", events.dump());
    let told = NoticeKind::PickNotUsed {
        model: Some("GPT-6".to_owned()),
        effort: Some("Medium".to_owned()),
    };
    assert_eq!(notices, [told], "{}", events.dump());
    host.shutdown();
}

/// The text of every notice logged on the thread, Clauseo's and the agent's.
pub(crate) fn notice_texts(events: &Events) -> Vec<String> {
    (events.events())
        .filter_map(|event| match event {
            clauseo_protocol::DomainEvent::Notice(notice) => Some(notice.text.clone()),
            _ => None,
        })
        .collect()
}

/// Replayed from the live proof (`pickers-kept-resumed-notice`): codex-acp resumes the thread on
/// its default and warns that it was recorded with gpt-6-luna; the start sets gpt-6-luna and low
/// back, and no notice of any kind reaches the thread.
#[test]
fn codex_acps_resume_warning_is_left_out_when_the_kept_model_is_set_back() {
    let dir = Dir::new(
        "pickers_kept_resumed_notice",
        &[(CODEX, Scenario::Local("pickers-kept-resumed-notice"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = idle_thread(&host, matter, Some(&LUNA_LOW));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let second = run(&host, &mut events, thread, "And clause 5?");
    host.settle();
    assert_eq!(
        started_and_notices(&events, second).0,
        names("GPT-6 Luna", "Low")
    );
    assert_eq!(
        notice_texts(&events),
        Vec::<String>::new(),
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// Any other warning codex-acp sends at the start still shows (`pickers-kept-other-notice`).
#[test]
fn another_warning_at_the_start_still_shows() {
    let dir = Dir::new(
        "pickers_kept_other_notice",
        &[(CODEX, Scenario::Local("pickers-kept-other-notice"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = idle_thread(&host, matter, Some(&LUNA_LOW));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    run(&host, &mut events, thread, "And clause 5?");
    host.settle();
    assert_eq!(
        notice_texts(&events),
        ["Codex could not refresh its plugins; try again later."],
        "{}",
        events.dump()
    );
    host.shutdown();
}
