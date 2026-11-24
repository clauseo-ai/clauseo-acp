//! The pickers across an idle stop (R20b): a Claude conversation sent on sonnet and low, whose
//! agent then stops idle, gets sonnet and low set again before its next prompt when the
//! relaunch loads the session at the agent's defaults (opus and medium, under the AIR
//! `recommendedValue` capability), and the header the pickers read shows them
//! (`pickers-idle-stop`, which takes each prompt only after both sets). And a reload whose
//! answer already holds the thread's values keeps them over the defaults the agent reported
//! before that answer (`pickers-load-held`).

use clauseo_protocol::{
    ConfigOption, StopReason, StreamHeader, StreamId, ThreadId, ThreadStatus, ViewPatchBody,
};

use crate::codex::CODEX;
use crate::idle_stop::{LIMIT, idle_stopped};
use crate::pickers::{CLAUDE, create_thread, current, logged, picks, replaced, turn};
use crate::support::{Dir, Running, Scenario};

/// The options in the header of a fresh subscription's first frame: what the pickers show.
fn header_options(host: &Running, thread: ThreadId) -> Vec<ConfigOption> {
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until_items(1);
    let first = view.patches().next().expect("a first frame");
    match &first.body {
        ViewPatchBody::Reset { view_window } => match &view_window.header {
            Some(StreamHeader::Thread(header)) => header.config_options.clone(),
            other => panic!("no thread header: {other:?}"),
        },
        other => panic!("the first frame is not a reset: {other:?}"),
    }
}

#[test]
fn picks_are_set_again_after_an_idle_stop_and_the_header_shows_them() {
    let dir = Dir::new(
        "pickers_idle_stop",
        &[(CLAUDE, Scenario::Local("pickers-idle-stop"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let config = picks(&[("model", "sonnet"), ("effort", "low")]);
    let thread = create_thread(&host, matter, Some(config));
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise clause 4.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    assert_eq!(
        current(&logged(&events), "model").as_deref(),
        Some("sonnet")
    );

    host.wait_armed(LIMIT);
    host.clock.advance(LIMIT);
    events.until("the idle stop", |events| {
        events.position(idle_stopped).is_some()
    });
    host.settle();
    assert_eq!(host.status(matter, thread), ThreadStatus::IdleStopped);

    let (second, _) = host.prompt(thread, "And clause 5?");
    let ended = events.until_turn_ended(second);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some("sonnet"));
    assert_eq!(current(&options, "effort").as_deref(), Some("low"));
    let shown = header_options(&host, thread);
    assert_eq!(current(&shown, "model").as_deref(), Some("sonnet"));
    assert_eq!(current(&shown, "effort").as_deref(), Some("low"));
    host.shutdown();
}

/// A reopened conversation whose `session/load` answer already holds its model and effort
/// (opus and high) needs no set; the options update at the defaults the agent sent during the
/// load, before that answer, is older than it and is dropped there, so the header the pickers
/// read never shows the defaults, during the reload or after.
#[test]
fn a_reload_that_holds_the_threads_values_never_shows_the_defaults_sent_before_it() {
    let dir = Dir::new(
        "pickers_load_held",
        &[(CLAUDE, Scenario::Bundled("pickers-picked"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let config = picks(&[("model", "opus"), ("effort", "high")]);
    let thread = create_thread(&host, matter, Some(config));
    let first = replaced(&turn(&host, thread, "Summarise clause 4.")).len();
    host.shutdown();

    dir.set_agents(&[(CLAUDE, Scenario::Local("pickers-load-held"))]);
    let host = dir.start(1);
    let events = turn(&host, thread, "And clause 5?");
    let reload = replaced(&events).split_off(first);
    assert!(!reload.is_empty(), "{}", events.dump());
    for options in &reload {
        assert_eq!(
            current(options, "model").as_deref(),
            Some("opus"),
            "{}",
            events.dump()
        );
        assert_eq!(current(options, "effort").as_deref(), Some("high"));
    }
    let shown = header_options(&host, thread);
    assert_eq!(current(&shown, "model").as_deref(), Some("opus"));
    assert_eq!(current(&shown, "effort").as_deref(), Some("high"));
    assert_eq!(current(&logged(&events), "model").as_deref(), Some("opus"));
    host.shutdown();
}

/// A Codex thread on gpt-6-luna and low (its `CreateThread` config) that ran one prompt, stopped
/// idle, and ran a second prompt to its end.
fn codex_relaunch(test: &str, scenario: &'static str) -> Relaunched {
    let dir = Dir::new(test, &[(CODEX, Scenario::Local(scenario))]);
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let outcome = host.ok(clauseo_protocol::HostCommand::CreateThread {
        matter,
        agent: clauseo_protocol::AgentId::new(CODEX).unwrap(),
        config: Some(picks(&[
            ("model", "gpt-6-luna"),
            ("reasoning_effort", "low"),
        ])),
    });
    let clauseo_protocol::CommandOutcome::ThreadCreated { thread } = outcome else {
        panic!("CreateThread answered {outcome:?}");
    };
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise clause 4.");
    assert_eq!(
        events.until_turn_ended(first).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    host.wait_armed(LIMIT);
    host.clock.advance(LIMIT);
    events.until("the idle stop", |events| {
        events.position(idle_stopped).is_some()
    });
    host.settle();
    assert_eq!(host.status(matter, thread), ThreadStatus::IdleStopped);
    let (second, _) = host.prompt(thread, "And clause 5?");
    let ended = events.until_turn_ended(second);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    Relaunched {
        _dir: dir,
        host,
        thread,
        events,
        second,
    }
}

/// What [`codex_relaunch`] leaves: the folder (kept while the host runs), the host, the
/// thread, its events and the second turn.
struct Relaunched {
    _dir: Dir,
    host: Running,
    thread: ThreadId,
    events: crate::observe::Events,
    second: clauseo_protocol::TurnId,
}

/// `second`'s `TurnStarted` model and effort names, and the `PickNotUsed` notices of `events`.
fn started_and_notices(
    events: &crate::observe::Events,
    second: clauseo_protocol::TurnId,
) -> (
    (Option<String>, Option<String>),
    Vec<clauseo_protocol::NoticeKind>,
) {
    use clauseo_protocol::{DomainEvent, NoticeKind};
    let started = events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnStarted(started) if started.turn == second => Some((
                started.model.as_ref().map(|m| m.name.to_string()),
                started.effort.as_ref().map(|e| e.name.to_string()),
            )),
            _ => None,
        })
        .expect("the second turn started");
    let notices = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => notice.kind.clone(),
            _ => None,
        })
        .filter(|kind| matches!(kind, NoticeKind::PickNotUsed { .. }))
        .collect();
    (started, notices)
}

/// (a) Codex's shape: the relaunch after an idle stop sets the kept model and effort back
/// before the second prompt, and the header shows them.
#[test]
fn a_codex_relaunch_sets_the_kept_model_and_effort_back() {
    let Relaunched {
        host,
        thread,
        events,
        second,
        ..
    } = codex_relaunch("pickers_codex_idle", "pickers-codex-idle");
    let shown = header_options(&host, thread);
    assert_eq!(current(&shown, "model").as_deref(), Some("gpt-6-luna"));
    assert_eq!(current(&shown, "reasoning_effort").as_deref(), Some("low"));
    let (started, notices) = started_and_notices(&events, second);
    assert_eq!(
        started,
        (Some("GPT-6 Luna".to_owned()), Some("Low".to_owned()))
    );
    assert!(notices.is_empty(), "{}", events.dump());
    host.shutdown();
}

/// (b) The relaunch's session no longer lists the kept model: the turn runs on what the session
/// holds, the lawyer is told once, naming it, and the header shows it.
#[test]
fn a_codex_relaunch_that_lost_the_kept_model_is_told_once() {
    let Relaunched {
        host,
        thread,
        events,
        second,
        ..
    } = codex_relaunch("pickers_codex_idle_gone", "pickers-codex-idle-gone");
    let shown = header_options(&host, thread);
    assert_eq!(current(&shown, "model").as_deref(), Some("gpt-6"));
    assert_eq!(current(&shown, "reasoning_effort").as_deref(), Some("low"));
    let (started, notices) = started_and_notices(&events, second);
    assert_eq!(
        started,
        (Some("GPT-6".to_owned()), Some("Low".to_owned())),
        "{}",
        events.dump()
    );
    assert_eq!(
        notices,
        [clauseo_protocol::NoticeKind::PickNotUsed {
            model: Some("GPT-6".to_owned()),
            effort: Some("Low".to_owned()),
        }],
        "{}",
        events.dump()
    );
    host.shutdown();
}
