//! The rail's triage at the interface (0045): `SnoozeThread`, `MarkThreadDone` and `SweepDone`
//! validate their times against the host's clock, log their events, show on the thread's row,
//! and keep across a restart; the morning sweep marks exactly the quiet threads, once.

use std::time::Duration;

use clauseo_protocol::{
    CommandOutcome, DomainEvent, DoneBy, ErrorCode, HostCommand, MAX_DAY_START_SKEW_MS,
    MAX_SNOOZE_AHEAD_MS, MatterId, SnoozeBy, StreamId, ThreadId, ThreadSummary, Timestamp,
};

use crate::queue::{DURABLE, held, until_queued};
use crate::support::{Dir, FAKE, Running, START_MS, Scenario, matter_with_thread};

const PERM: &str = "perm-agent";
const SLOW: &str = "slow-agent";
const SEND: &str = "send-agent";

fn at(ms: i64) -> Timestamp {
    Timestamp::new(ms).unwrap()
}

/// `thread`'s row in `matter`'s list.
pub(crate) fn row(host: &Running, matter: MatterId, thread: ThreadId) -> ThreadSummary {
    host.threads(matter)
        .into_iter()
        .find(|row| row.thread == thread)
        .unwrap_or_else(|| panic!("{thread} is not listed"))
}

pub(crate) fn snooze(thread: ThreadId, until: Option<i64>) -> HostCommand {
    HostCommand::SnoozeThread {
        thread,
        until: until.map(at),
    }
}

/// How many `thread_done` events by the morning sweep `thread`'s log holds.
fn swept(host: &Running, thread: ThreadId) -> usize {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    events.count(|event| {
        matches!(event, DomainEvent::ThreadDone(done) if done.by == DoneBy::MorningSweep && done.done)
    })
}

#[test]
fn snooze_and_done_validate_log_show_and_survive_a_restart() {
    let dir = Dir::new(
        "triage_commands",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    // Not after now, or more than 366 days ahead: refused, nothing logged.
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let head = events.head();
    for until in [START_MS, START_MS - 1, START_MS + MAX_SNOOZE_AHEAD_MS + 1] {
        host.refused(snooze(thread, Some(until)), ErrorCode::CommandInvalid);
    }
    host.refused(
        HostCommand::SweepDone {
            day_start: at(START_MS + MAX_DAY_START_SKEW_MS + 1),
        },
        ErrorCode::CommandInvalid,
    );
    host.refused(
        HostCommand::SweepDone {
            day_start: at(START_MS - MAX_DAY_START_SKEW_MS - 1),
        },
        ErrorCode::CommandInvalid,
    );
    let stranger = ThreadId::new(crate::support::foreign_uuid(7)).unwrap();
    host.refused(snooze(stranger, None), ErrorCode::CommandUnknownThread);
    host.settle();
    events.drain_ready();
    assert_eq!(events.head(), head, "{}", events.dump());

    // The edges pass; the last snooze and done stand.
    let far = START_MS + MAX_SNOOZE_AHEAD_MS;
    assert_eq!(
        host.ok(snooze(thread, Some(START_MS + 1))),
        CommandOutcome::Done
    );
    assert_eq!(host.ok(snooze(thread, Some(far))), CommandOutcome::Done);
    let done = HostCommand::MarkThreadDone { thread, done: true };
    assert_eq!(host.ok(done), CommandOutcome::Done);
    events.until("both triage events", |e| {
        e.count(|event| matches!(event, DomainEvent::ThreadDone(_))) == 1
    });
    let snoozes: Vec<_> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ThreadSnoozed(snoozed) => Some((snoozed.until, snoozed.by)),
            _ => None,
        })
        .collect();
    assert_eq!(
        snoozes,
        [
            (Some(at(START_MS + 1)), SnoozeBy::Lawyer),
            (Some(at(far)), SnoozeBy::Lawyer)
        ]
    );
    host.settle();
    let shown = row(&host, matter, thread);
    assert_eq!(shown.snoozed_until, Some(at(far)), "{shown:?}");
    assert!(shown.done, "{shown:?}");
    host.shutdown();

    // A restart folds the log again: both stand.
    let host = dir.start(1);
    let shown = row(&host, matter, thread);
    assert_eq!(shown.snoozed_until, Some(at(far)), "{shown:?}");
    assert!(shown.done, "{shown:?}");
    // Cleared by hand.
    host.ok(snooze(thread, None));
    host.ok(HostCommand::MarkThreadDone {
        thread,
        done: false,
    });
    host.settle();
    let shown = row(&host, matter, thread);
    assert_eq!(
        (shown.snoozed_until, shown.done),
        (None, false),
        "{shown:?}"
    );
    host.shutdown();
}

#[test]
fn the_morning_sweep_marks_exactly_the_quiet_threads_once() {
    let dir = Dir::new(
        "triage_sweep",
        &[
            (FAKE, Scenario::Bundled("prompt-stream")),
            (PERM, Scenario::Bundled("permission-kinds")),
            (SLOW, Scenario::Bundled("cancel-unanswered")),
            (SEND, Scenario::Bundled("send-now")),
        ],
    );
    let host = dir.start(0);
    // Each busy thread in a matter of its own, since one turn runs per matter at a time.
    let mut matters = Vec::new();
    let mut matter = |name: &str| {
        let matter = host.create_matter(name, &dir.folder(name, &[("brief.txt", "x\n")]));
        matters.push(matter);
        matter
    };
    let (queue_matter, perm_matter, slow_matter, quiet) =
        (matter("q"), matter("p"), matter("s"), matter("m"));

    // A queued prompt waits, held after a cancel, with no turn running.
    let queued = host.create_thread(queue_matter, SEND);
    let mut send_events = host.events(StreamId::Thread(queued));
    let (first, _) = host.prompt(queued, "Find the cap.");
    host.wait_armed(DURABLE);
    let (second, _) = host.prompt(queued, "Use the 2019 version instead");
    until_queued(&mut send_events, second);
    host.ok(HostCommand::Cancel { thread: queued });
    send_events.until_turn_ended(first);
    host.settle();
    assert!(held(&host, queue_matter, queued), "the queue is not held");
    // A permission waits.
    let asking = host.create_thread(perm_matter, PERM);
    let mut perm_events = host.events(StreamId::Thread(asking));
    let (asking_turn, _) = host.prompt(asking, "Edit the brief.");
    perm_events.until("a permission request", |e| {
        !e.permission_requests().is_empty()
    });
    // A turn runs.
    let running = host.create_thread(slow_matter, SLOW);
    let mut slow_events = host.events(StreamId::Thread(running));
    let (running_turn, _) = host.prompt(running, "Read the clause.");
    slow_events.until("the turn's start", |e| {
        e.count(|event| matches!(event, DomainEvent::TurnStarted(_))) > 0
    });
    // Quiet: never prompted; and answered and seen.
    let fresh = host.create_thread(quiet, FAKE);
    let seen = host.create_thread(quiet, FAKE);
    let (turn, _) = host.prompt(seen, "Summarise the brief.");
    host.events(StreamId::Thread(seen)).until_turn_ended(turn);
    host.ok(HostCommand::MarkSeen { thread: seen });
    // Answered, not seen.
    let unseen = host.create_thread(quiet, FAKE);
    let (turn, _) = host.prompt(unseen, "Summarise the brief.");
    host.events(StreamId::Thread(unseen)).until_turn_ended(turn);
    // Snoozed past the sweep.
    let snoozed = host.create_thread(quiet, FAKE);
    host.ok(snooze(snoozed, Some(START_MS + 48 * 3_600_000)));
    // Snoozed until a time that passes before the sweep: woken at it, then awake.
    let woken = host.create_thread(quiet, FAKE);
    host.ok(snooze(woken, Some(START_MS + 10_000)));
    host.settle();
    host.wait_armed(Duration::from_secs(10));
    let mut woken_events = host.events(StreamId::Thread(woken));
    host.clock.advance(Duration::from_secs(10));
    woken_events.until("the wake", |e| {
        e.count(|event| matches!(event, DomainEvent::ThreadSnoozed(s) if s.by == SnoozeBy::Expired))
            == 1
    });
    host.settle();
    assert_eq!(row(&host, quiet, woken).snoozed_until, None);
    // Already done.
    let done = host.create_thread(quiet, FAKE);
    host.ok(HostCommand::MarkThreadDone {
        thread: done,
        done: true,
    });
    // Active after the day began.
    host.clock.advance(Duration::from_secs(60));
    let late = host.create_thread(quiet, FAKE);
    host.settle();

    let day_start = at(START_MS + 30_000);
    assert_eq!(
        host.ok(HostCommand::SweepDone { day_start }),
        CommandOutcome::Done
    );
    host.settle();
    let mut marked: Vec<ThreadId> = matters
        .iter()
        .flat_map(|&matter| host.threads(matter))
        .filter(|row| row.done)
        .map(|row| row.thread)
        .collect();
    marked.sort();
    let mut want = vec![fresh, seen, woken, done];
    want.sort();
    assert_eq!(
        marked, want,
        "fresh {fresh} seen {seen} woken {woken} done {done}"
    );
    for thread in [fresh, seen, woken] {
        assert_eq!(swept(&host, thread), 1, "{thread}");
    }
    for thread in [done, unseen, asking, running, queued, snoozed, late] {
        assert_eq!(swept(&host, thread), 0, "{thread}");
    }
    // A second sweep the same day logs nothing new.
    assert_eq!(
        host.ok(HostCommand::SweepDone { day_start }),
        CommandOutcome::Done
    );
    host.settle();
    for thread in [fresh, seen, woken] {
        assert_eq!(swept(&host, thread), 1, "{thread}");
    }
    // Both live turns end before the shutdown, each by its 5 s cancel fallback (neither agent
    // answers the cancel), so the shutdown has no fallback of its own to wait for: with two
    // fallbacks armed at different moments, moving the clock once could miss the second.
    host.ok(HostCommand::Cancel { thread: running });
    host.ok(HostCommand::Cancel { thread: asking });
    host.wait_armed(Duration::from_secs(5));
    host.wait_armed(Duration::from_secs(5));
    host.clock.advance(Duration::from_secs(5));
    slow_events.until_turn_ended(running_turn);
    perm_events.until_turn_ended(asking_turn);
    host.shutdown();
}

#[test]
fn the_sweep_never_marks_an_archived_thread_done() {
    let dir = Dir::new(
        "triage_sweep_archived",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (matter, archived, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let quiet = host.create_thread(matter, FAKE);
    host.ok(HostCommand::ArchiveThread { thread: archived });
    host.clock.advance(Duration::from_secs(60));
    host.settle();
    let day_start = at(START_MS + 30_000);
    host.ok(HostCommand::SweepDone { day_start });
    host.settle();
    assert!(
        row(&host, matter, quiet).done,
        "the quiet thread is not swept"
    );
    assert!(
        !row(&host, matter, archived).done,
        "the archived thread was swept"
    );
    assert_eq!(swept(&host, archived), 0);
    host.shutdown();

    // After a restart its header is not known until something loads it; still not swept.
    let host = dir.start(1);
    let day_start = at(START_MS + 3_600_000 + 30_000);
    host.clock.advance(Duration::from_secs(60));
    host.ok(HostCommand::SweepDone { day_start });
    host.settle();
    assert_eq!(swept(&host, archived), 0);
    host.shutdown();
}
