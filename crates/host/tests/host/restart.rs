//! Start-up and restarts: reconciliation of an interrupted turn, the store lock, the Scratch
//! matter, and `session/load` with replay dedupe.

use clauseo_host::Host;
use clauseo_protocol::{
    ContentBlock, DomainEvent, ErrorCode, ExpiryReason, MatterKind, StopReason, StreamId,
};

use crate::observe::Events;
use crate::permissions::{PERM, until_first_request};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread};

/// Each envelope as (seq, kind), for comparing a stream across starts.
fn log(events: &Events) -> Vec<(u64, String)> {
    events
        .seen
        .iter()
        .map(|e| (e.seq.get(), e.event.kind().to_owned()))
        .collect()
}

#[test]
fn reconcile_ends_interrupted_turn_once() {
    let dir = Dir::new(
        "reconcile_ends_interrupted_turn_once",
        &[(PERM, Scenario::Bundled("permission-kinds"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], PERM);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the four clauses.");
    until_first_request(&mut events);
    let request = events.permission_requests()[0];
    drop(events);
    host.abort();

    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let mut matter_events = host.events(StreamId::Matter(matter));
    matter_events.until_caught_up();
    let expired = events.count(|event| {
        matches!(event, DomainEvent::PermissionExpired(p)
            if p.request == request && p.reason == ExpiryReason::HostRestarted)
    });
    let failed = events.count(|event| {
        matches!(event, DomainEvent::TurnFailed(f)
            if f.turn == turn && f.code == ErrorCode::ThreadInterrupted)
    });
    let ended = events.count(|e| matches!(e, DomainEvent::TurnEnded(t) if t.turn == turn));
    assert_eq!((expired, failed, ended), (1, 1, 1), "{}", events.dump());
    host.shutdown();
    events.until_end();
    matter_events.until_end();
    let (thread_log, matter_log) = (log(&events), log(&matter_events));

    let host = dir.start(2);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let mut matter_events = host.events(StreamId::Matter(matter));
    matter_events.until_caught_up();
    assert_eq!(
        log(&events),
        thread_log,
        "the third start logged on the thread"
    );
    assert_eq!(
        log(&matter_events),
        matter_log,
        "the third start logged on the matter"
    );
    host.shutdown();
}

#[test]
fn second_start_on_same_folder_is_locked() {
    let dir = Dir::new("second_start_on_same_folder_is_locked", &[]);
    let host = dir.start(0);
    let clock = Dir::clock(1);
    let error = Host::start(dir.config(1, &clock)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::StoreLocked, "{error}");
    assert!(!error.retryable());
    host.shutdown();
}

#[test]
fn scratch_matter_created_once() {
    let dir = Dir::new("scratch_matter_created_once", &[]);
    let created = |events: &Events| {
        events.count(|event| {
            matches!(event, DomainEvent::MatterCreated(m) if m.kind() == MatterKind::Scratch)
        })
    };

    let host = dir.start(0);
    let matters = host.matters();
    assert_eq!(matters.len(), 1, "{matters:?}");
    assert_eq!(matters[0].kind, MatterKind::Scratch);
    let scratch = matters[0].matter;
    let mut events = host.events(StreamId::Matter(scratch));
    events.until_caught_up();
    assert_eq!(created(&events), 1, "{}", events.dump());
    host.shutdown();

    let host = dir.start(1);
    let matters = host.matters();
    assert_eq!(matters.len(), 1, "a second Scratch matter: {matters:?}");
    assert_eq!(matters[0].matter, scratch);
    let mut events = host.events(StreamId::Matter(scratch));
    events.until_caught_up();
    assert_eq!(created(&events), 1, "{}", events.dump());
    host.shutdown();
}

/// Counts of the events a replay could log twice: by `acp_message_id` for text, by `acp_id`
/// for tool calls, and user messages holding `Tighten clause 4`.
fn replay_counts(events: &Events) -> [usize; 4] {
    let text = |id: &str| {
        events.count(|event| {
            matches!(event, DomainEvent::TextAppended(t) if t.acp_message_id.as_deref() == Some(id))
        })
    };
    let tool = events.count(|event| {
        matches!(event, DomainEvent::ToolCallStarted(s) if s.acp_id.as_str() == "tc-edit-0")
    });
    let user = events.count(|event| {
        matches!(event, DomainEvent::UserMessage(m) if m.content.iter().any(|block| {
            matches!(block, ContentBlock::Text { text } if text == "Tighten clause 4")
        }))
    });
    [text("m-thought-0"), text("m-answer-0"), tool, user]
}

/// Run 0 plays `replay-seed`, a live turn with the ids `load-replay` replays. Run 1 plays
/// `replay-load`: it accepts only `session/load` of `sess-1` (anything else is a mismatch),
/// replays that turn, then answers one more prompt.
#[test]
fn resume_with_load_drops_replayed_updates() {
    let dir = Dir::new(
        "resume_with_load_drops_replayed_updates",
        &[(FAKE, Scenario::Local("replay-seed"))],
    );
    let host = dir.start(0);
    let (_, thread, _) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4 draft\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Tighten clause 4");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    events.assert_no_mismatch();
    assert_eq!(replay_counts(&events), [1, 1, 1, 1], "{}", events.dump());
    host.shutdown();

    dir.set_agents(&[(FAKE, Scenario::Local("replay-load"))]);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (second, _) = host.prompt(thread, "And clause 5?");
    let ended = events.until_turn_ended(second);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(second), "Second answer.");
    assert_eq!(
        replay_counts(&events),
        [1, 1, 1, 1],
        "replayed updates logged again: {}",
        events.dump()
    );
    host.shutdown();
}
