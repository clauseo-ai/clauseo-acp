//! A message stopped or removed before its turn started: it leaves the thread's last stop as it
//! was, and a thread with nothing else to show leaves the rail's lists (`ListThreads` and the
//! app's `ThreadsChanged`), across a restart too. A thread whose first message waits is listed,
//! and a turn that failed before it started still counts as a stop.
//! Conversations answer at the same time, so a message waits in its own thread's queue behind a
//! turn held on a permission, or, for a thread's first message, while its agent never answers
//! `initialize` (`idle`).

use std::time::Duration;

use clauseo_host::Clock as _;
use clauseo_protocol::{
    CommandOutcome, HostCommand, HostEvent, LastStop, MatterId, StopReason, StreamId, ThreadId,
    ThreadSummary,
};

use crate::observe::started;
use crate::permissions::{KINDS_ANSWERS, PERM, answer_from, until_first_request};
use crate::settings::{ready, subscribe_app};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

/// An agent that answers its first prompt and holds its second on `perm-1`.
const ASKS: &str = "asks-agent";
/// An agent that never answers `initialize`: a turn of its stays unstarted.
const SILENT: &str = "silent-agent";

fn dir(test: &str) -> Dir {
    Dir::new(
        test,
        &[
            (PERM, Scenario::Bundled("permission-kinds")),
            (FAKE, Scenario::Bundled("prompt-stream")),
            (ASKS, Scenario::Local("answer-then-ask")),
            (SILENT, Scenario::Bundled("idle")),
        ],
    )
}

/// `thread`'s row in `ListThreads`, if listed.
fn row(host: &Running, matter: MatterId, thread: ThreadId) -> Option<ThreadSummary> {
    host.threads(matter)
        .into_iter()
        .find(|row| row.thread == thread)
}

/// (1) B answered once (`end_turn`); its second turn waits on a permission, and its third
/// message, queued behind it, is removed: B's last stop is still the first answer's.
#[test]
fn a_stopped_queued_message_keeps_the_earlier_stop() {
    let dir = dir("unstarted_keeps_earlier_stop");
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "Clause 4 draft\n")]);
    let matter = host.create_matter("m", &folder);
    let b = host.create_thread(matter, ASKS);
    let mut events_b = host.events(StreamId::Thread(b));
    let (first, _) = host.prompt(b, "Summarise the indemnity clause.");
    assert_eq!(events_b.until_turn_ended(first).stop, StopReason::EndTurn);
    host.settle();
    let answered = row(&host, matter, b).unwrap().last_stop.unwrap();
    assert_eq!(answered.reason, StopReason::EndTurn);

    host.clock.advance(Duration::from_secs(60));
    let (second, _) = host.prompt(b, "Edit the cap.");
    until_first_request(&mut events_b);
    let (third, _) = host.prompt(b, "And the schedule.");
    host.settle();
    let remove = HostCommand::RemoveQueuedPrompt {
        thread: b,
        turn: third,
    };
    assert_eq!(host.ok(remove), CommandOutcome::Done);
    let ended = events_b.until_turn_ended(third);
    assert_eq!(ended.stop, StopReason::Cancelled);
    assert_eq!(events_b.count(|event| started(event, third)), 0);
    host.settle();
    let row_b = row(&host, matter, b).expect("B answered once: it stays listed");
    assert_eq!(row_b.last_stop, Some(answered), "{}", events_b.dump());
    assert_ne!(answered.at, host.clock.now().wall);

    answer_from(&host, &mut events_b, b, 0, &["allow-once"]);
    assert_eq!(events_b.until_turn_ended(second).stop, StopReason::EndTurn);
    host.settle();
    host.shutdown();
}

/// (2) A prompt queued in a thread whose turn runs (held on `perm-1`), then removed: no last
/// stop.
#[test]
fn a_removed_queued_prompt_sets_no_stop() {
    let dir = dir("unstarted_removed_no_stop");
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "Clause 4 draft\n")]);
    let matter = host.create_matter("m", &folder);
    let a = host.create_thread(matter, PERM);
    let mut events_a = host.events(StreamId::Thread(a));
    let (turn_a, _) = host.prompt(a, "Edit the four clauses.");
    until_first_request(&mut events_a);
    let (queued, _) = host.prompt(a, "Then the schedule.");
    host.settle();
    let remove = HostCommand::RemoveQueuedPrompt {
        thread: a,
        turn: queued,
    };
    assert_eq!(host.ok(remove), CommandOutcome::Done);
    let ended = events_a.until_turn_ended(queued);
    assert_eq!(ended.stop, StopReason::Cancelled);
    assert_eq!(events_a.count(|event| started(event, queued)), 0);
    host.settle();
    let row_a = row(&host, matter, a).expect("A's turn runs: it is listed");
    assert_eq!(row_a.last_stop, None, "{}", events_a.dump());

    answer_from(&host, &mut events_a, a, 0, &KINDS_ANSWERS);
    events_a.until_turn_ended(turn_a);
    host.settle();
    host.shutdown();
}

/// (3) A new thread whose first message waits (its agent never answers `initialize`) is
/// listed; stopped before its turn starts, it leaves `ListThreads` and the app's list, and
/// stays out after a restart. A thread that answered, in the same matter, stays listed.
#[test]
fn a_thread_whose_only_message_was_stopped_is_not_listed() {
    let dir = dir("unstarted_blank_unlisted");
    let host = dir.start(0);
    let (mut sub, _) = subscribe_app(&host);
    let folder = dir.folder("m", &[("brief.txt", "Clause 4 draft\n")]);
    let matter = host.create_matter("m", &folder);
    let a = host.create_thread(matter, FAKE);
    let mut events_a = host.events(StreamId::Thread(a));
    let (turn_a, _) = host.prompt(a, "Summarise the indemnity clause.");
    assert_eq!(events_a.until_turn_ended(turn_a).stop, StopReason::EndTurn);
    let b = host.create_thread(matter, SILENT);
    let mut events_b = host.events(StreamId::Thread(b));
    let (turn_b, _) = host.prompt(b, "Read the brief.");
    host.settle();
    let waiting = row(&host, matter, b).expect("a first message that waits is listed");
    assert_eq!(waiting.last_stop, None);
    let sent = ready(&mut sub);
    assert!(
        sent.iter().any(|event| matches!(
            event,
            HostEvent::ThreadsChanged(changed)
                if changed.threads.iter().any(|row| row.thread == b)
        )),
        "B's row went out: {sent:?}"
    );
    assert!(
        !sent.iter().any(|event| matches!(
            event,
            HostEvent::ThreadsChanged(changed) if changed.removed.contains(&b)
        )),
        "B never left while its message was on its way: {sent:?}"
    );

    assert_eq!(
        host.ok(HostCommand::Cancel { thread: b }),
        CommandOutcome::Done
    );
    let ended = events_b.until_turn_ended(turn_b);
    assert_eq!(ended.stop, StopReason::Cancelled, "{}", events_b.dump());
    assert_eq!(events_b.count(|event| started(event, turn_b)), 0);
    host.settle();
    assert_eq!(row(&host, matter, b), None, "{}", events_b.dump());
    assert!(row(&host, matter, a).is_some());
    let sent = ready(&mut sub);
    let removed: Vec<ThreadId> = sent
        .iter()
        .filter_map(|event| match event {
            HostEvent::ThreadsChanged(changed) if changed.matter == matter => {
                Some(changed.removed.clone())
            }
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(removed, [b], "{sent:?}");

    drop(events_a);
    drop(events_b);
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

/// (4) A first turn that fails before it starts still ends with a stop.
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
