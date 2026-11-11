//! A turn the agent starts on its own, cancelled or cut short: after a Send now, what the agent
//! still sends once it heard `session/cancel` (a late edit, tool call and text) is dropped, and
//! the prompt sent now starts only once the agent has stopped, so none of it lands in the new
//! turn, whose Undo takes back only its own edit; and a host killed while such a turn is open
//! ends it at the next start as any interrupted turn, keeping what it logged.

use clauseo_protocol::{
    CommandOutcome, DeliveryRequest, DomainEvent, ErrorCode, FileChoice, FileDecision, HostCommand,
    MatterPath, StopReason, StreamId, ThreadId, TurnId,
};

use crate::observe::Events;
use crate::queue::{send, settle};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, text};

/// The agent-started turn's text in both scenarios.
const OWN_TEXT: &str = "The build finished: 3 warnings.";

/// The first turn that started other than `not`, once its `TurnStarted` is in.
fn until_other_start(events: &mut Events, not: TurnId) -> TurnId {
    let find = |e: &Events| {
        e.events().find_map(|event| match event {
            DomainEvent::TurnStarted(started) if started.turn != not => Some(started.turn),
            _ => None,
        })
    };
    events.until("an agent-started TurnStarted", |e| find(e).is_some());
    find(events).unwrap()
}

/// The tool calls of `turn` in `events`, by their ACP id.
fn tools_of(events: &Events, turn: TurnId) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ToolCallStarted(t) if t.turn == turn => Some(t.acp_id.to_string()),
            _ => None,
        })
        .collect()
}

/// The paths of the files `turn` changed, as its `FilesChanged` lists them.
fn changed_by(events: &Events, turn: TurnId) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::FilesChanged(changed) if changed.turn == turn => {
                Some(changed.files.clone())
            }
            _ => None,
        })
        .flatten()
        .map(|file| file.path().as_str().to_owned())
        .collect()
}

#[test]
fn send_now_drops_an_agent_started_turns_late_output() {
    let dir = Dir::new(
        "send_now_drops_an_agent_started_turns_late_output",
        &[(FAKE, Scenario::Local("own-turn-send-now-late"))],
    );
    let host = dir.start(0);
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Run the build.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    let own = until_other_start(&mut events, first);

    let (next, _) = send(
        &host,
        thread,
        text("Read clause 9"),
        DeliveryRequest::SendNow,
    );
    assert_eq!(events.until_turn_ended(own).stop, StopReason::Cancelled);
    // The agent's late output came before its stop signal, which let the prompt start.
    assert_eq!(events.until_turn_ended(next).stop, StopReason::EndTurn);
    settle(&host, &mut events);

    assert_eq!(
        events.turn_text(next),
        "Clause 9 says.",
        "{}",
        events.dump()
    );
    assert_eq!(tools_of(&events, next), ["tc-p2-1"], "{}", events.dump());
    assert_eq!(tools_of(&events, own), ["tc-own-1"], "{}", events.dump());
    assert!(!events.turn_text(own).contains("Late"), "{}", events.dump());
    let late_tools =
        events.count(|e| matches!(e, DomainEvent::ToolCallStarted(t) if t.acp_id == "tc-late-1"));
    assert_eq!(late_tools, 0, "{}", events.dump());
    // No other turn opened for the late output.
    let starts = events.count(|e| matches!(e, DomainEvent::TurnStarted(_)));
    assert_eq!(starts, 3, "{}", events.dump());

    // The new turn's step holds its own edit only: the late one came before its checkpoint.
    assert_eq!(
        changed_by(&events, next),
        ["answer.txt"],
        "{}",
        events.dump()
    );
    let outcome = host.ok(HostCommand::UndoStep {
        thread,
        turn: next,
        files: vec![FileDecision {
            path: MatterPath::new("answer.txt").unwrap(),
            choice: FileChoice::Undo,
        }],
    });
    assert!(
        matches!(outcome, CommandOutcome::StepUndone { .. }),
        "{outcome:?}"
    );
    events.until("StepUndone", |e| {
        e.events()
            .any(|event| matches!(event, DomainEvent::StepUndone(_)))
    });
    assert!(
        !folder.join("answer.txt").exists(),
        "the step's own edit is undone"
    );
    assert_eq!(
        std::fs::read_to_string(folder.join("late.txt")).unwrap(),
        "A late edit.\n",
        "the late edit is not the step's"
    );
    host.shutdown();
}

/// The thread's events of a fresh start, caught up.
fn caught_up(host: &Running, thread: ThreadId) -> Events {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    events
}

#[test]
fn a_restart_ends_an_open_agent_started_turn_and_keeps_its_entries() {
    let dir = Dir::new(
        "a_restart_ends_an_open_agent_started_turn_and_keeps_its_entries",
        &[(FAKE, Scenario::Local("own-turn-restart"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Run the build.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    let own = until_other_start(&mut events, first);
    events.until("the agent-started turn's tool call", |e| {
        tools_of(e, own) == ["tc-own-1"]
    });
    // The tool call's commit carries the text before it.
    assert_eq!(events.turn_text(own), OWN_TEXT, "{}", events.dump());
    assert!(events.turn_ended(own).is_none(), "the turn is open");
    drop(events);
    host.abort();

    // The next start ends it once, as any interrupted turn, and keeps what it logged.
    let host = dir.start(1);
    let events = caught_up(&host, thread);
    let failed = events.count(|e| {
        matches!(e, DomainEvent::TurnFailed(f)
            if f.turn == own && f.code == ErrorCode::ThreadInterrupted)
    });
    let ended = events.count(|e| matches!(e, DomainEvent::TurnEnded(t) if t.turn == own));
    assert_eq!((failed, ended), (1, 1), "{}", events.dump());
    assert_eq!(events.turn_ended(own).unwrap().stop, StopReason::Failed);
    assert_eq!(tools_of(&events, own), ["tc-own-1"], "{}", events.dump());
    assert_eq!(events.turn_text(own), OWN_TEXT, "{}", events.dump());
    let users = events.count(|e| matches!(e, DomainEvent::UserMessage(m) if m.turn == own));
    assert_eq!(users, 0, "{}", events.dump());
    let count = events.events().count();
    drop(events);
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until_items(1);
    assert!(view.mentions(OWN_TEXT), "{}", view.dump());
    assert!(view.mentions("Read build.log"), "{}", view.dump());
    drop(view);
    host.shutdown();

    // A third start repairs nothing more.
    let host = dir.start(2);
    let events = caught_up(&host, thread);
    assert_eq!(events.events().count(), count, "{}", events.dump());
    drop(events);
    host.shutdown();
}
