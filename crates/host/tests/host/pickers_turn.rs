//! A prompt's own picks (`SendPrompt::config`) on a running session (`prompt_picks.rs`,
//! `prompt_picks/send.rs`): they are set before the turn's `TurnStarted`, which names what the
//! agent's answer holds, not what was picked (`pickers-turn-record`); a refused pick gets one
//! `PickNotUsed` notice; an effort pick is checked against the model pick's answer
//! (`pickers-turn-effort`); a steer that runs as its own turn keeps its picks
//! (`pickers-steer-fresh`) and one that joins the running answer drops them (`steer-injected`);
//! a turn put back for the sign-in keeps its picks (`pickers-sign-in-requeue`).

use clauseo_protocol::{
    AgentId, CommandOutcome, CompactString, DeliveryRequest, DomainEvent, HostCommand, NamedChoice,
    NoticeKind, StopReason, StreamId, ThreadId, TurnId, TurnStarted,
};

use crate::observe::Events;
use crate::pickers::picks;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, text, wait};

fn named(id: &str, name: &str) -> NamedChoice {
    NamedChoice {
        id: id.into(),
        name: name.into(),
    }
}

/// Sends `prompt` on `thread` with its own `config` and `delivery`; its turn.
fn send(
    host: &Running,
    thread: ThreadId,
    prompt: &str,
    (config, delivery): (&[(&str, &str)], DeliveryRequest),
) -> TurnId {
    let outcome = host.ok(HostCommand::SendPrompt {
        thread,
        content: text(prompt),
        delivery,
        config: Some(picks(config)),
    });
    let CommandOutcome::PromptAccepted { turn, .. } = outcome else {
        panic!("SendPrompt answered {outcome:?}");
    };
    turn
}

/// Runs `prompt` with its own `config` to its end; the thread's events so far.
fn run(host: &Running, thread: ThreadId, prompt: &str, config: &[(&str, &str)]) -> Events {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let turn = send(host, thread, prompt, (config, DeliveryRequest::Queue));
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    events
}

/// `turn`'s `TurnStarted`.
fn started(events: &Events, turn: TurnId) -> TurnStarted {
    events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnStarted(started) if started.turn == turn => Some(started.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no TurnStarted: {}", events.dump()))
}

/// The last turn of `events`.
fn last_turn(events: &Events) -> TurnId {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::TurnStarted(started) => Some(started.turn),
            _ => None,
        })
        .last()
        .expect("a turn started")
}

/// The `PickNotUsed` notices of `events`, in order.
fn not_used(events: &Events) -> Vec<NoticeKind> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => notice.kind.clone(),
            _ => None,
        })
        .filter(|kind| matches!(kind, NoticeKind::PickNotUsed { .. }))
        .collect()
}

#[test]
fn a_turn_names_what_the_agents_answer_holds_and_a_refused_pick_is_told() {
    let dir = Dir::new(
        "pickers_turn_record",
        &[(FAKE, Scenario::Local("pickers-turn-record"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    run(&host, thread, "One", &[]);
    // Sonnet alone: the agent gives it its own default effort, low, which the start names.
    let events = run(&host, thread, "Two", &[("model", "sonnet")]);
    let two = last_turn(&events);
    let start = started(&events, two);
    assert_eq!(start.model, Some(named("sonnet", "Sonnet 5.5")));
    assert_eq!(start.effort, Some(named("low", "Low")), "{}", events.dump());
    // The set's answer is logged before the turn starts.
    let answered = events
        .position(|event| {
            matches!(event, DomainEvent::ConfigOptionsReplaced(replaced)
                if replaced.options.iter().any(|option| matches!(&option.kind,
                    clauseo_protocol::ConfigOptionKind::Select { current_value, .. }
                        if current_value == "sonnet")))
        })
        .expect("the answer is logged");
    let start_at = events
        .position(|event| matches!(event, DomainEvent::TurnStarted(s) if s.turn == two))
        .unwrap();
    assert!(answered < start_at, "{}", events.dump());
    assert_eq!(not_used(&events), []);
    // Opus, refused: the start names what the session holds, and the lawyer is told once.
    let events = run(&host, thread, "Three", &[("model", "opus")]);
    let three = last_turn(&events);
    let start = started(&events, three);
    assert_eq!(start.model, Some(named("sonnet", "Sonnet 5.5")));
    assert_eq!(start.effort, Some(named("low", "Low")));
    assert_eq!(
        not_used(&events),
        [NoticeKind::PickNotUsed {
            model: Some("Sonnet 5.5".to_owned()),
            effort: Some("Low".to_owned()),
        }],
        "{}",
        events.dump()
    );
    let row = host.threads(matter).pop().unwrap();
    assert_eq!(row.model, Some(named("sonnet", "Sonnet 5.5")));
    host.shutdown();
}

#[test]
fn an_effort_pick_is_checked_against_the_model_picks_answer() {
    let dir = Dir::new(
        "pickers_turn_effort",
        &[(FAKE, Scenario::Local("pickers-turn-effort"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    run(&host, thread, "One", &[]);
    // Max, which only Sonnet offers.
    let events = run(
        &host,
        thread,
        "Two",
        &[("model", "sonnet"), ("effort", "max")],
    );
    let start = started(&events, last_turn(&events));
    assert_eq!(start.model, Some(named("sonnet", "Sonnet 5.5")));
    assert_eq!(start.effort, Some(named("max", "Max")), "{}", events.dump());
    run(&host, thread, "Three", &[("effort", "high")]);
    // High, the effort before the switch, which the switch reset.
    let events = run(
        &host,
        thread,
        "Four",
        &[("model", "opus"), ("effort", "high")],
    );
    let start = started(&events, last_turn(&events));
    assert_eq!(start.model, Some(named("opus", "Opus 5.5")));
    assert_eq!(
        start.effort,
        Some(named("high", "High")),
        "{}",
        events.dump()
    );
    assert!(not_used(&events).is_empty(), "{}", events.dump());
    host.shutdown();
}

#[test]
fn a_pick_answered_past_the_wait_is_told_and_its_late_answer_is_logged() {
    let dir = Dir::new(
        "pickers_turn_timeout",
        &[(FAKE, Scenario::Local("pickers-turn-timeout"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    run(&host, thread, "One", &[]);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let two = send(
        &host,
        thread,
        "Two",
        (&[("model", "sonnet")], DeliveryRequest::Queue),
    );
    // The set is out and unanswered: the wait runs out.
    host.wait_armed(std::time::Duration::from_secs(20));
    host.clock.advance(std::time::Duration::from_secs(21));
    assert_eq!(events.until_turn_ended(two).stop, StopReason::EndTurn);
    let sonnet = |event: &DomainEvent| {
        matches!(event, DomainEvent::ConfigOptionsReplaced(replaced)
            if replaced.options.iter().any(|option| matches!(&option.kind,
                clauseo_protocol::ConfigOptionKind::Select { current_value, .. }
                    if current_value == "sonnet")))
    };
    events.until("the late answer", |events| {
        events.position(sonnet).is_some()
    });
    events.assert_no_mismatch();
    // Two ran on what the thread held, and said so.
    let start = started(&events, two);
    assert_eq!(
        start.model,
        Some(named("opus", "Opus 5.5")),
        "{}",
        events.dump()
    );
    assert_eq!(
        not_used(&events),
        [NoticeKind::PickNotUsed {
            model: Some("Opus 5.5".to_owned()),
            effort: Some("High".to_owned()),
        }],
        "{}",
        events.dump()
    );
    let start_at = events
        .position(|event| matches!(event, DomainEvent::TurnStarted(s) if s.turn == two))
        .unwrap();
    assert!(
        events.position(sonnet).unwrap() > start_at,
        "{}",
        events.dump()
    );
    // The session is on sonnet now: Three's opus is set.
    let events = run(&host, thread, "Three", &[("model", "opus")]);
    let start = started(&events, last_turn(&events));
    assert_eq!(start.model, Some(named("opus", "Opus 5.5")));
    host.shutdown();
}

#[test]
fn a_steer_that_runs_as_its_own_turn_keeps_its_picks() {
    let dir = Dir::new(
        "pickers_steer_fresh",
        &[(FAKE, Scenario::Local("pickers-steer-fresh"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    // Nothing runs: the steer is a turn of its own.
    let turn = send(
        &host,
        thread,
        "Steered",
        (&[("model", "sonnet")], DeliveryRequest::Steer),
    );
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let config = events.events().find_map(|event| match event {
        DomainEvent::UserMessage(message) if message.turn == turn => Some(message.config.clone()),
        _ => None,
    });
    let config = config.expect("the steer's UserMessage");
    assert_eq!(
        config.get("model").map(CompactString::as_str),
        Some("sonnet"),
        "{}",
        events.dump()
    );
    assert_eq!(
        started(&events, turn).model,
        Some(named("sonnet", "Sonnet 5.5"))
    );
    assert!(not_used(&events).is_empty(), "{}", events.dump());
    host.shutdown();
}

#[test]
fn a_steer_that_joins_the_running_answer_drops_its_picks() {
    let dir = Dir::new(
        "pickers_steer_joins",
        &[(FAKE, Scenario::Bundled("steer-injected"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(crate::queue::DURABLE);
    let steered = send(
        &host,
        thread,
        "Focus on clause 4",
        (&[("model", "sonnet")], DeliveryRequest::Steer),
    );
    assert_eq!(steered, first, "the steer joins the running turn");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    events.assert_no_mismatch();
    let configs: Vec<_> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::UserMessage(message) if message.turn == first => {
                Some(message.config.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(configs.len(), 2, "{}", events.dump());
    assert!(
        configs.iter().all(std::collections::BTreeMap::is_empty),
        "a joining steer logs no picks: {configs:?}"
    );
    host.shutdown();
}

#[test]
fn a_turn_put_back_for_the_sign_in_keeps_its_picks() {
    let dir = Dir::new(
        "pickers_sign_in_requeue",
        &[(FAKE, Scenario::Local("pickers-sign-in-requeue"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    run(&host, thread, "One", &[]);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let two = send(
        &host,
        thread,
        "Two",
        (&[("effort", "high")], DeliveryRequest::Queue),
    );
    assert_eq!(events.until_turn_ended(two).stop, StopReason::Failed);
    let behind = |event: &DomainEvent| matches!(event, DomainEvent::TurnQueued(queued) if queued.behind_turn() == Some(two));
    events.until("the turn put back", |events| {
        events.position(behind).is_some()
    });
    let again = events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnQueued(queued) if queued.behind_turn() == Some(two) => {
                Some(queued.turn())
            }
            _ => None,
        })
        .unwrap();
    let config = events.events().find_map(|event| match event {
        DomainEvent::UserMessage(message) if message.turn == again => Some(message.config.clone()),
        _ => None,
    });
    let config = config.expect("the put-back turn's UserMessage");
    assert_eq!(
        config.get("effort").map(CompactString::as_str),
        Some("high"),
        "{}",
        events.dump()
    );
    host.settle();
    let fake = AgentId::new(FAKE).unwrap();
    wait("restart_agents", host.handle.restart_agents(fake)).unwrap();
    assert_eq!(events.until_turn_ended(again).stop, StopReason::EndTurn);
    events.assert_no_mismatch();
    assert_eq!(started(&events, again).effort, Some(named("high", "High")));
    host.shutdown();
}
