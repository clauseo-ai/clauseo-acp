//! A message stopped or removed before its turn started: it leaves the thread's last stop as it
//! was, and a thread with nothing else to show leaves the rail's lists (`ListThreads` and the
//! app's `ThreadsChanged`), across a restart too. A thread whose first message waits is listed,
//! and a turn that failed before it started still counts as a stop.

use std::time::Duration;

use clauseo_host::Clock as _;
use clauseo_protocol::{
    CommandOutcome, HostCommand, HostEvent, LastStop, MatterId, StopReason, StreamId, ThreadId,
    ThreadSummary,
};

use crate::permissions::{KINDS_ANSWERS, answer_from};
use crate::settings::{ready, subscribe_app};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};
use crate::writer::{TwoThreads, b_waits_behind_a, dir};

/// `thread`'s row in `ListThreads`, if listed.
fn row(host: &Running, matter: MatterId, thread: ThreadId) -> Option<ThreadSummary> {
    host.threads(matter)
        .into_iter()
        .find(|row| row.thread == thread)
}

/// Lets A's turn finish, so the writer is free and the host shuts down idle.
fn finish_a(host: &Running, t: &mut TwoThreads) {
    answer_from(host, &mut t.events_a, t.a, 0, &KINDS_ANSWERS);
    t.events_a.until_turn_ended(t.turn_a);
    host.settle();
}

/// (1) B answered once (`end_turn`); its second message waits behind A and is stopped: B's last
/// stop is still the first answer's.
#[test]
fn a_stopped_queued_message_keeps_the_earlier_stop() {
    let dir = dir("unstarted_keeps_earlier_stop");
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "Clause 4 draft\n")]);
    let matter = host.create_matter("m", &folder);
    let b = host.create_thread(matter, FAKE);
    let mut events_b = host.events(StreamId::Thread(b));
    let (first, _) = host.prompt(b, "Summarise the indemnity clause.");
    assert_eq!(events_b.until_turn_ended(first).stop, StopReason::EndTurn);
    host.settle();
    let answered = row(&host, matter, b).unwrap().last_stop.unwrap();
    assert_eq!(answered.reason, StopReason::EndTurn);

    host.clock.advance(Duration::from_secs(60));
    let mut t = b_waits_behind_a_with(&host, matter, b, events_b);
    assert_eq!(
        host.ok(HostCommand::Cancel { thread: b }),
        CommandOutcome::Done
    );
    let ended = t.events_b.until_turn_ended(t.turn_b);
    assert_eq!(ended.stop, StopReason::Cancelled);
    host.settle();
    let row_b = row(&host, matter, b).expect("B answered once: it stays listed");
    assert_eq!(row_b.last_stop, Some(answered), "{}", t.events_b.dump());
    assert_ne!(answered.at, host.clock.now().wall);
    finish_a(&host, &mut t);
    host.shutdown();
}

/// B's second message, waiting behind A's turn: [`b_waits_behind_a`] for a B that exists.
fn b_waits_behind_a_with(
    host: &Running,
    matter: MatterId,
    b: ThreadId,
    mut events_b: crate::observe::Events,
) -> TwoThreads {
    let a = host.create_thread(matter, crate::permissions::PERM);
    let mut events_a = host.events(StreamId::Thread(a));
    let (turn_a, _) = host.prompt(a, "Edit the four clauses.");
    crate::permissions::until_first_request(&mut events_a);
    let (turn_b, _) = host.prompt(b, "And the cap.");
    events_b.until("B's TurnQueued", |e| e.last_queued(turn_b).is_some());
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

/// (2) A prompt queued in a thread whose turn runs, then removed: no last stop.
#[test]
fn a_removed_queued_prompt_sets_no_stop() {
    let dir = dir("unstarted_removed_no_stop");
    let host = dir.start(0);
    let mut t = b_waits_behind_a(&dir, &host);
    let (queued, _) = host.prompt(t.a, "Then the schedule.");
    host.settle();
    let remove = HostCommand::RemoveQueuedPrompt {
        thread: t.a,
        turn: queued,
    };
    assert_eq!(host.ok(remove), CommandOutcome::Done);
    let ended = t.events_a.until_turn_ended(queued);
    assert_eq!(ended.stop, StopReason::Cancelled);
    host.settle();
    let row_a = row(&host, t.matter, t.a).expect("A's turn runs: it is listed");
    assert_eq!(row_a.last_stop, None, "{}", t.events_a.dump());
    finish_a(&host, &mut t);
    host.shutdown();
}

/// (3) and (4): a new thread whose first message waits is listed; stopped before its turn
/// starts, it leaves `ListThreads` and the app's list, and stays out after a restart.
#[test]
fn a_thread_whose_only_message_was_stopped_is_not_listed() {
    let dir = dir("unstarted_blank_unlisted");
    let host = dir.start(0);
    let (mut sub, _) = subscribe_app(&host);
    let mut t = b_waits_behind_a(&dir, &host);
    let waiting = row(&host, t.matter, t.b).expect("a first message that waits is listed");
    assert_eq!(waiting.last_stop, None);
    host.settle();
    let sent = ready(&mut sub);
    assert!(
        sent.iter().any(|event| matches!(
            event,
            HostEvent::ThreadsChanged(changed)
                if changed.threads.iter().any(|row| row.thread == t.b)
        )),
        "B's row went out: {sent:?}"
    );
    assert!(
        !sent.iter().any(|event| matches!(
            event,
            HostEvent::ThreadsChanged(changed) if changed.removed.contains(&t.b)
        )),
        "B never left while its message was on its way: {sent:?}"
    );

    assert_eq!(
        host.ok(HostCommand::Cancel { thread: t.b }),
        CommandOutcome::Done
    );
    t.events_b.until_turn_ended(t.turn_b);
    host.settle();
    assert_eq!(row(&host, t.matter, t.b), None, "{}", t.events_b.dump());
    assert!(row(&host, t.matter, t.a).is_some());
    let sent = ready(&mut sub);
    let removed: Vec<ThreadId> = sent
        .iter()
        .filter_map(|event| match event {
            HostEvent::ThreadsChanged(changed) if changed.matter == t.matter => {
                Some(changed.removed.clone())
            }
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(removed, [t.b], "{sent:?}");

    finish_a(&host, &mut t);
    let (matter, a, b) = (t.matter, t.a, t.b);
    drop(t);
    drop(sub);
    host.shutdown();

    let host = dir.start(1);
    assert_eq!(row(&host, matter, b), None, "a restart lists it again");
    assert!(row(&host, matter, a).is_some());
    let (_sub, first) = subscribe_app(&host);
    let listed = first.iter().any(|event| {
        matches!(
            event,
            HostEvent::ThreadsChanged(changed) if changed.threads.iter().any(|row| row.thread == b)
        )
    });
    assert!(!listed, "{first:?}");
    host.shutdown();
}

/// (5) A first turn that fails before it starts still ends with a stop.
#[test]
fn an_unstarted_failed_turn_is_a_stop() {
    let dir = Dir::new(
        "unstarted_failed_is_a_stop",
        &[(FAKE, Scenario::Local("new-session-error"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the brief.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    host.settle();
    let row = row(&host, matter, thread).expect("a failed turn has a step: it is listed");
    assert_eq!(
        row.last_stop,
        Some(LastStop {
            reason: StopReason::Failed,
            at: host.clock.now().wall
        }),
        "{}",
        events.dump()
    );
    host.shutdown();
}
