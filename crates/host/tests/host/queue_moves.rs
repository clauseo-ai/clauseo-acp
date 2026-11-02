//! Moving and changing queued prompts at the interface: send now, a queued prompt sent now,
//! editing (the files stay) and removing, the refusals for a turn that is not queued, and a
//! queue that comes back held after a host restart.

use clauseo_protocol::{
    CommandOutcome, ContentBlock, Delivery, DeliveryRequest, DomainEvent, ErrorCode, FileRefId,
    HostCommand, PromptBlock, StopReason, StreamId, ThreadId, TurnId,
};

use crate::observe::{Events, ended_with, started};
use crate::permissions::until_first_request;
use crate::queue::{
    DURABLE, allow, edited, held, queued_at, send, settle, starts, until_queued, user_message,
};
use crate::support::{Dir, FAKE, Running, Scenario, foreign_uuid, matter_with_thread, text, wait};

/// The text an edit gives the queued prompt in `queue-edit-file` and `queue-restart`.
const EDITED: &str = "Read the 2019 lease instead";

/// A prompt of `words` and the file `file`.
fn with_file(words: &str, file: FileRefId) -> Vec<PromptBlock> {
    vec![
        PromptBlock::Text {
            text: words.to_owned(),
        },
        PromptBlock::File { file },
    ]
}

/// Registers `path` as a file ref, as the desktop's file picker does.
fn register(host: &Running, path: std::path::PathBuf) -> FileRefId {
    wait("register_file", host.handle.register_file(path)).unwrap()
}

/// Checks a send-now run of `send-now`: `first` cancelled by the agent, `second` (sent now or
/// moved first) queued behind it in the one `TurnQueued`, then run to `EndTurn`; turn 1's text
/// kept.
fn assert_sent_now(events: &Events, thread: ThreadId, first: TurnId, second: TurnId) {
    let queued = events.last_queued(second).unwrap();
    assert_eq!(
        (queued.behind(), queued.behind_turn()),
        (thread, Some(first))
    );
    let queues = events.count(|e| matches!(e, DomainEvent::TurnQueued(q) if q.turn() == second));
    assert_eq!(queues, 1, "{}", events.dump());
    let first_end = events
        .position(|e| ended_with(e, first, StopReason::Cancelled))
        .unwrap();
    let second_start = events.position(|e| started(e, second)).unwrap();
    assert!(first_end < second_start, "{}", events.dump());
    assert_eq!(events.turn_text(first), "The clause caps liability at ");
    assert_eq!(events.turn_text(second), "Using the 2019 version. ");
}

#[test]
fn send_now_cancels_and_runs_next() {
    let dir = Dir::new(
        "send_now_cancels_and_runs_next",
        &[(FAKE, Scenario::Bundled("send-now"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Find the cap.");
    host.wait_armed(DURABLE);

    let (second, message) = send(
        &host,
        thread,
        text("Use the 2019 version instead"),
        DeliveryRequest::SendNow,
    );
    assert_ne!(second, first);
    // The agent answers the cancel: no clock move, so the 5 s fallback never fires.
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    let (user_at, user) = user_message(&events, message);
    assert_eq!((user.turn, user.delivery), (second, Delivery::SendNow));
    let queued_at = queued_at(&events, second);
    assert_eq!(
        events.batch[user_at],
        events.batch[queued_at],
        "{}",
        events.dump()
    );
    assert_sent_now(&events, thread, first, second);
    host.shutdown();
}

#[test]
fn send_queued_now_moves_it_first() {
    let dir = Dir::new(
        "send_queued_now_moves_it_first",
        &[(FAKE, Scenario::Bundled("send-now"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Find the cap.");
    host.wait_armed(DURABLE);
    let (second, message) = host.prompt(thread, "Use the 2019 version instead");
    until_queued(&mut events, second);
    let before = events.head();

    assert_eq!(
        host.ok(HostCommand::SendQueuedNow {
            thread,
            turn: second
        }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(user_message(&events, message).1.delivery, Delivery::Queue);
    // The move logged nothing of its own: nothing after the TurnQueued names turn 2 until it
    // starts.
    let named = events
        .seen
        .iter()
        .filter(|envelope| envelope.seq.get() > before)
        .take_while(|envelope| !started(&envelope.event, second))
        .filter(|envelope| {
            matches!(&envelope.event,
                DomainEvent::UserMessage(u) if u.turn == second)
                || matches!(&envelope.event,
                    DomainEvent::TurnQueued(q) if q.turn() == second)
                || matches!(&envelope.event, DomainEvent::QueuedPromptEdited(_))
        })
        .count();
    assert_eq!(named, 0, "{}", events.dump());
    assert_sent_now(&events, thread, first, second);
    host.shutdown();
}

#[test]
fn edit_keeps_files_and_remove_drops_it() {
    let dir = Dir::new(
        "edit_keeps_files_and_remove_drops_it",
        &[(FAKE, Scenario::Local("queue-edit-file"))],
    );
    let host = dir.start(0);
    let files = [("brief.txt", "Clause 4\n"), ("lease.pdf", "%PDF-1.4\n")];
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &files, FAKE);
    let file = register(&host, folder.join("lease.pdf"));
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Check clause 4.");
    until_first_request(&mut events);

    let (kept, kept_message) = send(
        &host,
        thread,
        with_file("Read the lease", file),
        DeliveryRequest::Queue,
    );
    let (removed, _) = host.prompt(thread, "Also check clause 9");
    until_queued(&mut events, removed);
    let original = user_message(&events, kept_message).1.content;
    let link = original[1].clone();
    assert!(
        matches!(&link, ContentBlock::ResourceLink { name, .. } if name == "lease.pdf"),
        "{original:?}"
    );

    let edit = HostCommand::EditQueuedPrompt {
        thread,
        turn: kept,
        text: EDITED.to_owned(),
    };
    assert_eq!(host.ok(edit), CommandOutcome::Done);
    events.until("QueuedPromptEdited", |e| edited(e, kept).is_some());
    let edit = edited(&events, kept).unwrap();
    assert_eq!(edit.message, kept_message);
    assert_eq!(
        edit.content,
        [
            ContentBlock::Text {
                text: EDITED.to_owned()
            },
            link
        ]
    );

    let remove = HostCommand::RemoveQueuedPrompt {
        thread,
        turn: removed,
    };
    assert_eq!(host.ok(remove), CommandOutcome::Done);
    let ended = events.until_turn_ended(removed);
    assert_eq!(
        (ended.stop, ended.checkpoint_after),
        (StopReason::Cancelled, None)
    );

    allow(&host, &mut events, thread);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    assert_eq!(events.until_turn_ended(kept).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(events.turn_text(kept), "Lease read.");
    assert_eq!(starts(&events, removed), 0, "{}", events.dump());
    let ends = events.count(|e| matches!(e, DomainEvent::TurnEnded(t) if t.turn == removed));
    assert_eq!(ends, 1, "{}", events.dump());
    host.shutdown();
}

#[test]
fn turn_not_queued_for_a_started_or_unknown_turn() {
    let dir = Dir::new(
        "turn_not_queued_for_a_started_or_unknown_turn",
        &[(FAKE, Scenario::Bundled("queue-two-prompts"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Check clause 4.");
    until_first_request(&mut events);
    settle(&host, &mut events);
    let head = events.head();

    let unknown = TurnId::new(foreign_uuid(7)).unwrap();
    for turn in [first, unknown] {
        let not_queued = ErrorCode::ThreadTurnNotQueued;
        let edit = HostCommand::EditQueuedPrompt {
            thread,
            turn,
            text: "Shorter".to_owned(),
        };
        host.refused(edit, not_queued);
        host.refused(HostCommand::RemoveQueuedPrompt { thread, turn }, not_queued);
        host.refused(HostCommand::SendQueuedNow { thread, turn }, not_queued);
    }
    settle(&host, &mut events);
    assert_eq!(events.head(), head, "a refusal logged: {}", events.dump());

    let (second, _) = host.prompt(thread, "Second question");
    until_queued(&mut events, second);
    let empty = HostCommand::EditQueuedPrompt {
        thread,
        turn: second,
        text: String::new(),
    };
    host.refused(empty, ErrorCode::CommandInvalid);

    allow(&host, &mut events, thread);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(events.turn_text(second), "Second answer.");
    host.shutdown();
}

/// Run 0 plays `queue-restart`'s `session/new` branch: prompt 1 waits on a permission while an
/// edited prompt with a file is queued, then the host is aborted. Run 1 resumes with
/// `session/load`, and the agent fails unless prompt 2 is the edited one with its file.
#[test]
fn restart_keeps_the_queue_held() {
    let dir = Dir::new(
        "restart_keeps_the_queue_held",
        &[(FAKE, Scenario::Local("queue-restart"))],
    );
    let host = dir.start(0);
    let files = [("brief.txt", "Clause 4\n"), ("lease.pdf", "%PDF-1.4\n")];
    let (matter, thread, folder) = matter_with_thread(&dir, &host, "m", &files, FAKE);
    let file = register(&host, folder.join("lease.pdf"));
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Check clause 4.");
    until_first_request(&mut events);
    let (queued, _) = send(
        &host,
        thread,
        with_file("Read the lease", file),
        DeliveryRequest::Queue,
    );
    let edit = HostCommand::EditQueuedPrompt {
        thread,
        turn: queued,
        text: EDITED.to_owned(),
    };
    assert_eq!(host.ok(edit), CommandOutcome::Done);
    events.until("QueuedPromptEdited", |e| edited(e, queued).is_some());
    drop(events);
    host.abort();

    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let interrupted = events.count(|event| {
        matches!(event, DomainEvent::TurnFailed(f)
            if f.turn == first && f.code == ErrorCode::ThreadInterrupted)
    });
    let ended = events.count(|e| matches!(e, DomainEvent::TurnEnded(t) if t.turn == first));
    assert_eq!((interrupted, ended), (1, 1), "{}", events.dump());
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "the queue is not held");
    assert_eq!(starts(&events, queued), 0, "{}", events.dump());
    assert!(events.turn_ended(queued).is_none(), "{}", events.dump());

    assert_eq!(
        host.ok(HostCommand::ResumeQueue { thread }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(queued).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(events.turn_text(queued), "Lease read.");
    assert!(!held(&host, matter, thread));
    host.shutdown();
}
