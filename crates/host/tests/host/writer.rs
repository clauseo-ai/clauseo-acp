//! One writer per matter: a second thread's turn waits behind the first's, is cancelled while
//! it waits, and survives a crash as a held queued turn.

use clauseo_protocol::{
    CommandOutcome, HostCommand, MatterId, StopReason, StreamId, ThreadId, ThreadStatus, TurnId,
};

use crate::observe::{Events, started};
use crate::permissions::{KINDS_ANSWERS, PERM, answer_from, until_first_request};
use crate::support::{Dir, FAKE, Running, Scenario};

/// Thread A (`permission-kinds`) holds the writer at `perm-1`; thread B (`prompt-stream`) in
/// the same matter has sent a prompt whose turn waits behind A.
struct TwoThreads {
    matter: MatterId,
    a: ThreadId,
    b: ThreadId,
    turn_a: TurnId,
    turn_b: TurnId,
    events_a: Events,
    events_b: Events,
}

fn dir(test: &str) -> Dir {
    Dir::new(
        test,
        &[
            (PERM, Scenario::Bundled("permission-kinds")),
            (FAKE, Scenario::Bundled("prompt-stream")),
        ],
    )
}

/// Builds [`TwoThreads`] and checks B's `TurnQueued { behind: A }`.
fn b_waits_behind_a(dir: &Dir, host: &Running) -> TwoThreads {
    let folder = dir.folder("m", &[("brief.txt", "Clause 4 draft\n")]);
    let matter = host.create_matter("m", &folder);
    let a = host.create_thread(matter, PERM);
    let b = host.create_thread(matter, FAKE);
    let mut events_a = host.events(StreamId::Thread(a));
    let mut events_b = host.events(StreamId::Thread(b));
    let (turn_a, _) = host.prompt(a, "Edit the four clauses.");
    until_first_request(&mut events_a);
    let (turn_b, _) = host.prompt(b, "Summarise the indemnity clause.");
    events_b.until("B's TurnQueued", |e| e.last_queued(turn_b).is_some());
    let queued = events_b.last_queued(turn_b).unwrap();
    assert_eq!(queued.behind(), a, "{}", events_b.dump());
    assert_eq!(events_b.count(|event| started(event, turn_b)), 0);
    TwoThreads {
        matter,
        a,
        b,
        turn_a,
        turn_b,
        events_a,
        events_b,
    }
}

#[test]
fn second_thread_queues_behind_writer() {
    let dir = dir("second_thread_queues_behind_writer");
    let host = dir.start(0);
    let mut t = b_waits_behind_a(&dir, &host);

    answer_from(&host, &mut t.events_a, t.a, 0, &KINDS_ANSWERS[..3]);
    host.settle();
    t.events_b.drain_ready();
    assert_eq!(
        t.events_b.count(|event| started(event, t.turn_b)),
        0,
        "B started while A held the writer: {}",
        t.events_b.dump()
    );
    answer_from(&host, &mut t.events_a, t.a, 3, &KINDS_ANSWERS[3..]);
    let ended_a = t.events_a.until_turn_ended(t.turn_a);
    assert_eq!(ended_a.stop, StopReason::EndTurn);

    let ended_b = t.events_b.until_turn_ended(t.turn_b);
    assert_eq!(ended_b.stop, StopReason::EndTurn, "{}", t.events_b.dump());
    let queued = t.events_b.position(|event| {
        matches!(event, clauseo_protocol::DomainEvent::TurnQueued(q) if q.turn() == t.turn_b)
    });
    let began = t.events_b.position(|event| started(event, t.turn_b));
    assert!(
        matches!((queued, began), (Some(q), Some(s)) if q < s),
        "{}",
        t.events_b.dump()
    );
    t.events_a.assert_no_mismatch();
    t.events_b.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn cancel_queued_turn_ends_once() {
    let dir = dir("cancel_queued_turn_ends_once");
    let host = dir.start(0);
    let mut t = b_waits_behind_a(&dir, &host);

    assert_eq!(
        host.ok(HostCommand::Cancel { thread: t.b }),
        CommandOutcome::Done
    );
    let ended_b = t.events_b.until_turn_ended(t.turn_b);
    assert_eq!(ended_b.stop, StopReason::Cancelled);
    assert_eq!(ended_b.checkpoint_after, None);

    answer_from(&host, &mut t.events_a, t.a, 0, &KINDS_ANSWERS);
    t.events_a.until_turn_ended(t.turn_a);
    host.settle();
    t.events_b.drain_ready();
    let turn_b = t.turn_b;
    assert_eq!(t.events_b.count(|event| started(event, turn_b)), 0);
    let ends = t.events_b.count(
        |event| matches!(event, clauseo_protocol::DomainEvent::TurnEnded(e) if e.turn == turn_b),
    );
    assert_eq!(ends, 1, "{}", t.events_b.dump());
    t.events_a.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn restart_holds_writer_waiting_turn() {
    let dir = dir("restart_holds_writer_waiting_turn");
    let host = dir.start(0);
    let t = b_waits_behind_a(&dir, &host);
    let (matter, b, turn_b) = (t.matter, t.b, t.turn_b);
    drop(t);
    host.abort();

    let host = dir.start(1);
    let mut events_b = host.events(StreamId::Thread(b));
    events_b.until_caught_up();
    let queued = events_b
        .last_queued(turn_b)
        .unwrap_or_else(|| panic!("no TurnQueued: {}", events_b.dump()));
    assert_eq!(queued.behind(), b, "{}", events_b.dump());
    assert_eq!(queued.behind_turn(), None);
    assert_eq!(events_b.count(|event| started(event, turn_b)), 0);
    assert_eq!(host.status(matter, b), ThreadStatus::Idle);
    host.shutdown();
}
