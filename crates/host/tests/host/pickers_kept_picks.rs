//! A thread's kept model and effort (`actor/turn/picks/own.rs`), with the stand-in agent as
//! Codex: a new thread's prefs that its first list lacks are kept and told once
//! (`pickers-kept-created-unlisted`); a model pick keeps the effort the agent answered with,
//! picked at once (`pickers-kept-effort-picked`) or while the session starts
//! (`pickers-kept-effort-waiting`), then `pickers-kept-effort-restored`; and a set-back that
//! fails with another error than -32602 is told once, with codex-acp's resume warning left out
//! (`pickers-kept-setback-failed`). An effort picked on a fallback list leaves the kept model
//! (`pickers-kept-fallback-effort`, then `pickers-kept-luna-high-back`), and a new thread's
//! effort its first list lacks beside a model it lacks is kept with the model
//! (`pickers-kept-created-effort-unlisted`); when the model, back, does not offer it, it is not
//! told and stays kept for a list that offers it (`pickers-kept-effort-unoffered`, then
//! `pickers-kept-luna-ultra-back`); an effort picked while a fallback relaunch starts leaves
//! the kept model too (`pickers-kept-fallback-effort-waiting`).

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};

use clauseo_protocol::{
    AgentKind, AgentPrefs, CommandOutcome, FieldChange, HostCommand, MatterId, NoticeKind,
    StopReason, StreamId,
};

use crate::codex::CODEX;
use crate::idle_stop::LIMIT;
use crate::matters::summary;
use crate::pickers::{picks, set};
use crate::pickers_early::{Gate, sent};
use crate::pickers_idle::started_and_notices;
use crate::pickers_kept::{LUNA_LOW, created, idle_thread, names, notice_texts, run, stop_idle};
use crate::support::{Dir, Running, Scenario, send_prompt, wait};

/// The matter's prefs for Codex: `pairs`.
fn codex_prefs(pairs: &[(&str, &str)]) -> AgentPrefs {
    AgentPrefs(BTreeMap::from([(AgentKind::Codex, picks(pairs))]))
}

/// Sets `matter`'s prefs to `prefs`.
fn set_prefs(host: &Running, matter: MatterId, prefs: AgentPrefs) {
    host.ok(HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        skill_packs: FieldChange::Keep,
        pinned: None,
        agent_prefs: FieldChange::Set(prefs),
    });
}

/// The one notice of a session that runs `model` and `effort` instead of the kept values.
fn told(model: &str, effort: &str) -> NoticeKind {
    NoticeKind::PickNotUsed {
        model: Some(model.to_owned()),
        effort: Some(effort.to_owned()),
    }
}

/// A new thread whose matter's prefs hold the model gpt-6-luna alone, and whose first list
/// lacks it: the thread keeps gpt-6-luna (the first session's medium fills the effort), the
/// first answer says once that it runs GPT-6, the prefs keep it, and the relaunch whose list has
/// it again sets it back with no other notice.
#[test]
fn a_new_threads_prefs_its_first_list_lacks_are_kept_and_told_once() {
    let dir = Dir::new(
        "pickers_kept_created_unlisted",
        &[(CODEX, Scenario::Local("pickers-kept-created-unlisted"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let luna = [("model", "gpt-6-luna")];
    set_prefs(&host, matter, codex_prefs(&luna));
    let thread = created(&host, matter, None);
    let mut events = host.events(StreamId::Thread(thread));
    let first = run(&host, &mut events, thread, "Summarise clause 4.");
    run(&host, &mut events, thread, "And clause 5?");
    let (started, notices) = started_and_notices(&events, first);
    assert_eq!(started, names("GPT-6", "Medium"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6", "Medium")], "{}", events.dump());
    assert_eq!(summary(&host, matter).agent_prefs, Some(codex_prefs(&luna)));
    stop_idle(&host, &mut events);

    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let third = run(&host, &mut events, thread, "And clause 6?");
    let (started, notices) = started_and_notices(&events, third);
    assert_eq!(started, names("GPT-6 Luna", "Medium"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6", "Medium")], "{}", events.dump());
    host.shutdown();
}

/// After a host restart, the thread's kept values are gpt-6-luna and high, the effort the agent
/// answered the pick with: the relaunch, on gpt-6 and low, sets gpt-6-luna back (which keeps
/// low), then high, with no notice; with medium kept instead, medium is not offered on
/// gpt-6-luna and the start would tell it (`pickers-kept-effort-restored`).
fn restored_on_luna_and_high(dir: &Dir, thread: clauseo_protocol::ThreadId) {
    dir.set_agents(&[(CODEX, Scenario::Local("pickers-kept-effort-restored"))]);
    let host = dir.start_with(1, |config| config.with_live_agents(2));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let next = run(&host, &mut events, thread, "And clause 6?");
    let (started, notices) = started_and_notices(&events, next);
    assert_eq!(started, names("GPT-6 Luna", "High"), "{}", events.dump());
    assert_eq!(notices, [], "{}", events.dump());
    host.shutdown();
}

/// The lawyer picks gpt-6-luna, which lacks the session's medium, so the agent answers with its
/// default high: the thread keeps gpt-6-luna and high, so no relaunch sets medium back or tells
/// that it could not.
#[test]
fn a_model_pick_keeps_the_effort_the_agent_answered_with() {
    let dir = Dir::new(
        "pickers_kept_effort_picked",
        &[(CODEX, Scenario::Local("pickers-kept-effort-picked"))],
    );
    let host = dir.start_with(0, |config| config.with_live_agents(2));
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = created(&host, matter, None);
    let mut events = host.events(StreamId::Thread(thread));
    run(&host, &mut events, thread, "Summarise clause 4.");
    assert_eq!(
        host.ok(set(thread, "model", "gpt-6-luna")),
        CommandOutcome::Done
    );
    host.shutdown();
    restored_on_luna_and_high(&dir, thread);
}

/// The same pick made while the relaunch after an idle stop starts (the launch is held until
/// it is sent): the start sets it, its command is answered then, and the thread keeps gpt-6-luna
/// and the high the agent answered with.
#[test]
fn a_model_pick_waiting_for_the_session_keeps_the_effort_the_agent_answered_with() {
    let dir = Dir::new(
        "pickers_kept_effort_waiting",
        &[(CODEX, Scenario::Local("pickers-kept-effort-waiting"))],
    );
    let open = Arc::new((Mutex::new(true), Condvar::new()));
    let gate = Gate {
        inner: dir.launcher(),
        open: Arc::clone(&open),
    };
    let host = dir.start_launching(0, Arc::new(gate), |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = idle_thread(&host, matter, None);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    *open.0.lock().unwrap() = false;

    let prompt = sent(&host, send_prompt(thread, "And clause 5?"));
    let pick = sent(&host, set(thread, "model", "gpt-6-luna"));
    Gate::open(&open);
    let CommandOutcome::PromptAccepted { turn, .. } = wait("the prompt", prompt).unwrap() else {
        panic!("SendPrompt answered another outcome");
    };
    assert_eq!(wait("the pick", pick).unwrap(), CommandOutcome::Done);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let (started, notices) = started_and_notices(&events, turn);
    assert_eq!(started, names("GPT-6 Luna", "High"), "{}", events.dump());
    assert_eq!(notices, [], "{}", events.dump());
    host.shutdown();
    restored_on_luna_and_high(&dir, thread);
}

/// The relaunch's set of the kept gpt-6-luna fails with an internal error (-32603), not a
/// refusal: the second answer says once that it runs GPT-6, the third nothing, codex-acp's
/// resume warning is left out (ours says it).
#[test]
fn a_set_back_that_fails_otherwise_is_told_once() {
    let dir = Dir::new(
        "pickers_kept_setback_failed",
        &[(CODEX, Scenario::Local("pickers-kept-setback-failed"))],
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
    let third = run(&host, &mut events, thread, "And clause 6?");
    host.settle();
    let (started, notices) = started_and_notices(&events, second);
    assert_eq!(started, names("GPT-6", "Low"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6", "Low")], "{}", events.dump());
    assert_eq!(started_and_notices(&events, third).0, names("GPT-6", "Low"));
    assert_eq!(notice_texts(&events).len(), 1, "{}", events.dump());
    host.shutdown();
}

/// On the fallback list (gpt-6 alone, the kept gpt-6-luna told once), the lawyer picks the effort
/// high: high is kept, and gpt-6-luna stays the kept model, not the gpt-6 the answer runs on. After
/// a host restart, the list has gpt-6-luna again and it is set back.
#[test]
fn an_effort_picked_on_a_fallback_list_leaves_the_kept_model() {
    let dir = Dir::new(
        "pickers_kept_fallback_effort",
        &[(CODEX, Scenario::Local("pickers-kept-fallback-effort"))],
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
    assert_eq!(
        started_and_notices(&events, second).1,
        [told("GPT-6", "Low")]
    );
    assert_eq!(
        host.ok(set(thread, "reasoning_effort", "high")),
        CommandOutcome::Done
    );
    host.shutdown();

    dir.set_agents(&[(CODEX, Scenario::Local("pickers-kept-luna-high-back"))]);
    let host = dir.start_with(1, |config| config.with_live_agents(2));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let third = run(&host, &mut events, thread, "And clause 6?");
    let (started, notices) = started_and_notices(&events, third);
    assert_eq!(started, names("GPT-6 Luna", "High"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6", "Low")], "{}", events.dump());
    host.shutdown();
}

/// A new thread whose matter's prefs hold gpt-6-luna and xhigh, and whose first list lacks both
/// (gpt-6, which offers up to high): both are kept and told once, the prefs keep both, and the
/// relaunch whose list has gpt-6-luna sets it, then xhigh, with no other notice.
#[test]
fn a_new_threads_effort_beside_a_model_its_first_list_lacks_is_kept_with_it() {
    let dir = Dir::new(
        "pickers_kept_created_effort",
        &[(
            CODEX,
            Scenario::Local("pickers-kept-created-effort-unlisted"),
        )],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let chosen = [("model", "gpt-6-luna"), ("reasoning_effort", "xhigh")];
    set_prefs(&host, matter, codex_prefs(&chosen));
    let thread = created(&host, matter, None);
    let mut events = host.events(StreamId::Thread(thread));
    let first = run(&host, &mut events, thread, "Summarise clause 4.");
    let (started, notices) = started_and_notices(&events, first);
    assert_eq!(started, names("GPT-6", "Medium"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6", "Medium")], "{}", events.dump());
    assert_eq!(
        summary(&host, matter).agent_prefs,
        Some(codex_prefs(&chosen))
    );
    stop_idle(&host, &mut events);

    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let second = run(&host, &mut events, thread, "And clause 5?");
    let (started, notices) = started_and_notices(&events, second);
    assert_eq!(
        started,
        names("GPT-6 Luna", "Extra high"),
        "{}",
        events.dump()
    );
    assert_eq!(notices, [told("GPT-6", "Medium")], "{}", events.dump());
    host.shutdown();
}

/// The lawyer's prefs hold gpt-6-luna and ultra. A new thread's first list lacks both, which are
/// kept and told once; the relaunch lists gpt-6-luna, whose efforts lack ultra there (a list that
/// may be a fallback): gpt-6-luna is set with no notice, and the thread and the prefs keep ultra.
/// After a host restart gpt-6-luna offers ultra, and both are set (`pickers-kept-luna-ultra-back`).
#[test]
fn a_kept_effort_the_running_model_does_not_offer_is_not_told_and_set_once_offered() {
    let dir = Dir::new(
        "pickers_kept_effort_unoffered",
        &[(CODEX, Scenario::Local("pickers-kept-effort-unoffered"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    set_prefs(
        &host,
        matter,
        codex_prefs(&[("model", "gpt-6-luna"), ("reasoning_effort", "ultra")]),
    );
    let thread = created(&host, matter, None);
    let mut events = host.events(StreamId::Thread(thread));
    let first = run(&host, &mut events, thread, "Summarise clause 4.");
    assert_eq!(
        started_and_notices(&events, first).1,
        [told("GPT-6", "Medium")]
    );
    stop_idle(&host, &mut events);

    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let second = run(&host, &mut events, thread, "And clause 5?");
    let (started, notices) = started_and_notices(&events, second);
    assert_eq!(started, names("GPT-6 Luna", "Medium"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6", "Medium")], "{}", events.dump());
    let chosen = [("model", "gpt-6-luna"), ("reasoning_effort", "ultra")];
    assert_eq!(
        summary(&host, matter).agent_prefs,
        Some(codex_prefs(&chosen))
    );
    host.shutdown();
    assert_eq!(
        crate::pickers_kept_rows::stored(&dir, thread).as_deref(),
        Some(r#"{"codex-acp":{"model":"gpt-6-luna","reasoning_effort":"ultra"}}"#)
    );

    dir.set_agents(&[(CODEX, Scenario::Local("pickers-kept-luna-ultra-back"))]);
    let host = dir.start_with(1, |config| config.with_live_agents(2));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let third = run(&host, &mut events, thread, "And clause 6?");
    let (started, notices) = started_and_notices(&events, third);
    assert_eq!(started, names("GPT-6 Luna", "Ultra"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6", "Medium")], "{}", events.dump());
    host.shutdown();
}

/// The lawyer picks the effort high while the fallback relaunch (gpt-6 alone) starts, the launch
/// held until it is sent: the start sets it and answers it, high is kept, and gpt-6-luna stays the
/// kept model; after a host restart the list has gpt-6-luna again and it is set back.
#[test]
fn an_effort_picked_while_a_fallback_relaunch_starts_leaves_the_kept_model() {
    let dir = Dir::new(
        "pickers_kept_fallback_effort_waiting",
        &[(
            CODEX,
            Scenario::Local("pickers-kept-fallback-effort-waiting"),
        )],
    );
    let open = Arc::new((Mutex::new(true), Condvar::new()));
    let gate = Gate {
        inner: dir.launcher(),
        open: Arc::clone(&open),
    };
    let host = dir.start_launching(0, Arc::new(gate), |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = idle_thread(&host, matter, Some(&LUNA_LOW));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    *open.0.lock().unwrap() = false;

    let prompt = sent(&host, send_prompt(thread, "And clause 5?"));
    let pick = sent(&host, set(thread, "reasoning_effort", "high"));
    Gate::open(&open);
    let CommandOutcome::PromptAccepted { turn, .. } = wait("the prompt", prompt).unwrap() else {
        panic!("SendPrompt answered another outcome");
    };
    assert_eq!(wait("the pick", pick).unwrap(), CommandOutcome::Done);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let (started, notices) = started_and_notices(&events, turn);
    assert_eq!(started, names("GPT-6", "High"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6", "High")], "{}", events.dump());
    host.shutdown();

    dir.set_agents(&[(CODEX, Scenario::Local("pickers-kept-luna-high-back"))]);
    let host = dir.start_with(1, |config| config.with_live_agents(2));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let third = run(&host, &mut events, thread, "And clause 6?");
    let (started, notices) = started_and_notices(&events, third);
    assert_eq!(started, names("GPT-6 Luna", "High"), "{}", events.dump());
    assert_eq!(notices, [told("GPT-6", "High")], "{}", events.dump());
    host.shutdown();
}
