//! A prompt's own picks (`SendPrompt::config`) across a restart: a prompt queued with effort
//! high behind a turn the host never finished is logged with its picks (`UserMessage::config`),
//! comes back queued and held after the restart, and once resumed the reloaded session gets
//! effort high just before that prompt (`pickers-queued-restart`), its `TurnStarted` naming it.

use clauseo_protocol::{
    CommandOutcome, DeliveryRequest, DomainEvent, HostCommand, NamedChoice, StopReason, StreamId,
};

use crate::pickers::{CLAUDE, picks};
use crate::queue::{held, settle, until_queued};
use crate::support::{Dir, Scenario, matter_with_thread, text};

#[test]
fn a_queued_prompts_pick_survives_a_restart() {
    let dir = Dir::new(
        "pickers_queued_restart",
        &[(CLAUDE, Scenario::Local("pickers-queued-restart"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(
        &dir,
        &host,
        "m",
        &[(
            "brief.txt",
            "x
",
        )],
        CLAUDE,
    );
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "One");
    events.until("TurnStarted", |e| {
        e.events()
            .any(|event| matches!(event, DomainEvent::TurnStarted(s) if s.turn == first))
    });
    let outcome = host.ok(HostCommand::SendPrompt {
        thread,
        content: text("Two"),
        delivery: DeliveryRequest::Queue,
        config: Some(picks(&[("effort", "high")])),
    });
    let CommandOutcome::PromptAccepted { turn: queued, .. } = outcome else {
        panic!("SendPrompt answered {outcome:?}");
    };
    until_queued(&mut events, queued);
    let logged = events.events().find_map(|event| match event {
        DomainEvent::UserMessage(message) if message.turn == queued => Some(message.config.clone()),
        _ => None,
    });
    let logged = logged.expect("the queued prompt's UserMessage");
    assert_eq!(
        logged
            .get("effort")
            .map(clauseo_protocol::CompactString::as_str),
        Some("high")
    );
    drop(events);
    host.abort();

    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "the queue comes back held");
    assert_eq!(
        host.ok(HostCommand::ResumeQueue { thread }),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(queued);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let started = events.events().find_map(|event| match event {
        DomainEvent::TurnStarted(started) if started.turn == queued => Some(started.clone()),
        _ => None,
    });
    let started = started.expect("the queued turn started");
    let high = NamedChoice {
        id: "high".into(),
        name: "High".into(),
    };
    assert_eq!(started.effort, Some(high));
    host.shutdown();
}
