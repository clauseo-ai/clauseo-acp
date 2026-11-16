//! A snooze wakes at its time with no client timer (0045): the host's timer logs
//! `thread_snoozed { by: expired }` and the thread's row is sent again without the snooze; a
//! changed snooze re-arms it, a cleared one cancels it, two wake in order, and a snooze that
//! ended while the host was down wakes at start-up.

use std::time::Duration;

use clauseo_host::AppSubscription;
use clauseo_protocol::{
    DomainEvent, HostCommand, HostEvent, SnoozeBy, StreamId, ThreadId, ThreadSummary, Timestamp,
};
use futures::StreamExt;

use crate::matters::{created, import};
use crate::observe::Events;
use crate::settings::{decode, ready, subscribe_app};
use crate::support::{Dir, FAKE, Running, START_MS, Scenario, matter_with_thread, wait};
use crate::triage::{row, snooze};
use crate::update::STANDING_TIMERS;

const MINUTE: Duration = Duration::from_secs(60);

fn dir(test: &str) -> Dir {
    Dir::new(test, &[(FAKE, Scenario::Bundled("prompt-stream"))])
}

/// How many wakes by the host `events` holds.
fn wakes(events: &Events) -> usize {
    events.count(|event| {
        matches!(event, DomainEvent::ThreadSnoozed(s) if s.by == SnoozeBy::Expired && s.until.is_none())
    })
}

/// `thread`'s events so far.
fn caught_up(host: &Running, thread: ThreadId) -> Events {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    events
}

/// Waits until `sub` sends a row of `thread` that `done` accepts.
fn until_row(
    sub: &mut AppSubscription,
    thread: ThreadId,
    what: &str,
    done: impl Fn(&ThreadSummary) -> bool,
) {
    wait(what, async {
        while let Some(item) = sub.next().await {
            if let HostEvent::ThreadsChanged(changed) = decode(&item)
                && changed
                    .threads
                    .iter()
                    .any(|row| row.thread == thread && done(row))
            {
                return;
            }
        }
        panic!("the app subscription ended");
    });
}

#[test]
fn an_idle_host_logs_the_wake_at_its_time_and_sends_the_row_again() {
    let dir = dir("snooze_wakes_idle");
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let until = START_MS + 10 * 60_000;
    host.ok(snooze(thread, Some(until)));
    host.settle();
    host.wait_armed(10 * MINUTE);
    let (mut sub, _) = subscribe_app(&host);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    // A minute short: nothing.
    host.clock.advance(9 * MINUTE);
    host.settle();
    events.drain_ready();
    assert_eq!(wakes(&events), 0, "{}", events.dump());
    // At its time: the wake, and the row without the snooze.
    host.clock.advance(MINUTE);
    events.until("the wake", |e| wakes(e) == 1);
    host.settle();
    assert_eq!(row(&host, matter, thread).snoozed_until, None);
    let sent: Vec<_> = ready(&mut sub)
        .into_iter()
        .filter_map(|event| match event {
            HostEvent::ThreadsChanged(changed) => Some(changed),
            _ => None,
        })
        .flat_map(|changed| changed.threads)
        .filter(|summary| summary.thread == thread)
        .collect();
    assert!(
        sent.last()
            .is_some_and(|summary| summary.snoozed_until.is_none()),
        "{sent:?}"
    );
    host.shutdown();
}

#[test]
fn a_changed_snooze_rearms_and_a_cleared_one_cancels() {
    let dir = dir("snooze_rearm_cancel");
    let host = dir.start(0);
    let (matter, moved, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let cleared = host.create_thread(matter, FAKE);
    host.ok(snooze(moved, Some(START_MS + 10 * 60_000)));
    host.settle();
    host.wait_armed(10 * MINUTE);
    host.ok(snooze(moved, Some(START_MS + 20 * 60_000)));
    host.settle();
    host.wait_armed(20 * MINUTE);
    host.clock.advance(10 * MINUTE);
    host.settle();
    assert_eq!(wakes(&caught_up(&host, moved)), 0);
    host.clock.advance(10 * MINUTE);
    caught_up(&host, moved).until("the moved wake", |e| wakes(e) == 1);

    // Cleared by hand: no timer left, and no wake later.
    host.ok(snooze(cleared, Some(START_MS + 30 * 60_000)));
    host.settle();
    host.wait_armed(10 * MINUTE);
    host.ok(snooze(cleared, None));
    host.settle();
    assert_eq!(
        host.clock.armed(),
        STANDING_TIMERS,
        "the timer is still armed"
    );
    host.clock.advance(20 * MINUTE);
    host.settle();
    assert_eq!(wakes(&caught_up(&host, cleared)), 0);
    host.shutdown();
}

#[test]
fn two_snoozes_wake_in_order() {
    let dir = dir("snooze_two_in_order");
    let host = dir.start(0);
    let (matter, first, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let second = host.create_thread(matter, FAKE);
    host.ok(snooze(second, Some(START_MS + 20 * 60_000)));
    host.ok(snooze(first, Some(START_MS + 10 * 60_000)));
    host.settle();
    host.wait_armed(10 * MINUTE);
    host.clock.advance(10 * MINUTE);
    caught_up(&host, first).until("the first wake", |e| wakes(e) == 1);
    host.settle();
    assert_eq!(wakes(&caught_up(&host, second)), 0);
    assert!(row(&host, matter, second).snoozed_until.is_some());
    host.wait_armed(10 * MINUTE);
    host.clock.advance(10 * MINUTE);
    caught_up(&host, second).until("the second wake", |e| wakes(e) == 1);
    host.settle();
    assert_eq!(wakes(&caught_up(&host, first)), 1);
    host.shutdown();
}

#[test]
fn a_snooze_that_ended_while_the_host_was_down_wakes_at_start_up() {
    let dir = dir("snooze_wakes_at_start_up");
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let later = host.create_thread(matter, FAKE);
    host.ok(snooze(thread, Some(START_MS + 10 * 60_000)));
    // Still ahead after the restart, which starts an hour on.
    let ahead = START_MS + 2 * 3_600_000;
    host.ok(snooze(later, Some(ahead)));
    host.settle();
    host.shutdown();

    let host = dir.start(1);
    caught_up(&host, thread).until("the start-up wake", |e| wakes(e) == 1);
    host.settle();
    assert_eq!(row(&host, matter, thread).snoozed_until, None);
    assert_eq!(
        row(&host, matter, later).snoozed_until,
        Some(Timestamp::new(ahead).unwrap())
    );
    assert_eq!(wakes(&caught_up(&host, later)), 0);
    host.shutdown();
}

// The reviewer's two repros (review of the H-triage pull request): a matter off the list and
// brought back has its threads loaded again, so its rows show their triage and a snooze that
// ended meanwhile wakes.

#[test]
fn review_a_snooze_that_ended_while_forgotten_wakes_at_import() {
    let dir = dir("review_snooze_import");
    let host = dir.start(0);
    let (matter, thread, folder) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let until = START_MS + 10 * 60_000;
    host.ok(snooze(thread, Some(until)));
    host.settle();
    host.wait_armed(10 * MINUTE);
    host.ok(HostCommand::ForgetMatter { matter });
    host.settle();
    host.clock.advance(15 * MINUTE);
    host.settle();
    assert_eq!(
        wakes(&caught_up(&host, thread)),
        0,
        "woken while off the list"
    );
    let back = created(import(&host, "m", &folder, (None, None)));
    assert_eq!(back, matter);
    caught_up(&host, thread).until("the wake after the import", |e| wakes(e) == 1);
    host.settle();
    assert_eq!(
        row(&host, matter, thread).snoozed_until,
        None,
        "past-due snooze {:?} not woken after import",
        Timestamp::new(until).unwrap()
    );
    assert_eq!(
        host.clock.armed(),
        STANDING_TIMERS,
        "a timer is still armed"
    );
    host.shutdown();
}

#[test]
fn review_done_shows_on_the_row_after_restart_and_import() {
    let dir = dir("review_done_import");
    let host = dir.start(0);
    let (matter, thread, folder) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    host.ok(HostCommand::MarkThreadDone { thread, done: true });
    host.settle();
    assert!(row(&host, matter, thread).done);
    host.ok(HostCommand::ForgetMatter { matter });
    host.settle();
    host.shutdown();
    let host = dir.start(1);
    let (mut sub, _) = subscribe_app(&host);
    let back = created(import(&host, "m", &folder, (None, None)));
    assert_eq!(back, matter);
    until_row(&mut sub, thread, "the row done after the import", |row| {
        row.done
    });
    assert!(
        row(&host, matter, thread).done,
        "done lost on the row after import"
    );
    host.shutdown();
}
