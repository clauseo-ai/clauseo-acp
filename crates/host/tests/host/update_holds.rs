//! An install's hold and the sign-in's hold on one thread at once (decision records 0037 and
//! 0048): a prompt sent while both apply queues held, and stays held until the last of them is
//! lifted, whichever goes first; then each queued prompt runs once, in order. The queue shows
//! the most specific reason it waits: the login switching over the install, the sign-in over the
//! install, and the install alone when nothing else holds it.

use clauseo_protocol::{
    AgentId, DeliveryRequest, DomainEvent, HostCommand, InstallWhen, QueueHoldReason, StopReason,
    StreamId,
};

use crate::observe::{Events, started};
use crate::queue::{DURABLE, held, hold_reason, send, starts};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, text, wait};
use crate::update::{FakeUpdater, start_updating, until_ready};

/// The agent of the thread both holds apply to; `stop_agents` holds it.
const HELD: &str = "fake-held";

/// Which hold is lifted first.
#[derive(Debug, Clone, Copy)]
enum First {
    Update,
    SignIn,
}

fn both_holds(test: &str, first: First, resume_queue: bool) {
    let dir = Dir::new(
        test,
        &[
            (FAKE, Scenario::Bundled("cancel-unanswered")),
            (HELD, Scenario::Local("update-two-prompts")),
        ],
    );
    let fake = FakeUpdater::offering(&dir);
    let host = start_updating(&dir, 0, &fake);
    let held_agent = AgentId::new(HELD).unwrap();
    let (_, running, _) = matter_with_thread(&dir, &host, "running", &[], FAKE);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "held", &[], HELD);
    let mut events = host.events(StreamId::Thread(thread));
    // A turn that never answers its cancel keeps the install's hold up.
    host.prompt(running, "Draft clause 4.");
    host.wait_armed(DURABLE);

    // The sign-in's hold, then the install's.
    wait("stop_agents", host.handle.stop_agents(held_agent.clone())).unwrap();
    host.ok(HostCommand::CheckForUpdate {});
    until_ready(&host);
    host.ok(HostCommand::InstallUpdate {
        when: InstallWhen::Now,
    });

    // Two prompts, the second sent now: both queue held, and nothing starts.
    let (one, _) = host.prompt(thread, "First question");
    let (two, _) = send(
        &host,
        thread,
        text("Second question"),
        DeliveryRequest::SendNow,
    );
    host.settle();
    events.drain_ready();
    let none_started =
        |events: &crate::observe::Events| (starts(events, one), starts(events, two)) == (0, 0);
    assert!(none_started(&events), "{}", events.dump());
    assert!(held(&host, matter, thread), "{}", events.dump());
    // The stop's hold, the sign-in's mark and the install's: the login switching shows.
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Switching)
    );

    // Under `stop_agents`' hold the lawyer's `ResumeQueue` changes nothing (#480).
    if resume_queue {
        host.ok(HostCommand::ResumeQueue { thread });
        host.settle();
        events.drain_ready();
        assert!(
            none_started(&events),
            "after ResumeQueue: {}",
            events.dump()
        );
        assert!(
            held(&host, matter, thread),
            "after ResumeQueue: {}",
            events.dump()
        );
    }

    // The first lift leaves the other hold: still nothing starts.
    let lift_update = || {
        host.ok(HostCommand::CancelScheduledUpdate {});
    };
    let lift_sign_in = || {
        wait(
            "restart_agents",
            host.handle.restart_agents(held_agent.clone()),
        )
        .unwrap();
    };
    match first {
        First::Update => lift_update(),
        First::SignIn => lift_sign_in(),
    }
    host.settle();
    events.drain_ready();
    assert!(none_started(&events), "{first:?} lifted: {}", events.dump());
    // The hold left shows: the install's alone holds turns no longer held for the lawyer.
    let left = match first {
        First::Update => QueueHoldReason::Switching,
        First::SignIn => QueueHoldReason::Update,
    };
    assert_eq!(hold_reason(&host, matter, thread), Some(left), "{first:?}");

    // The last lift: each runs once, in order.
    match first {
        First::Update => lift_sign_in(),
        First::SignIn => lift_update(),
    }
    assert_eq!(events.until_turn_ended(one).stop, StopReason::EndTurn);
    assert_eq!(events.until_turn_ended(two).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    assert_eq!((starts(&events, one), starts(&events, two)), (1, 1));
    let at = |turn| events.position(|e| started(e, turn)).unwrap();
    assert!(at(one) < at(two), "{}", events.dump());
    assert_eq!(hold_reason(&host, matter, thread), None);
    events.assert_no_mismatch();
    crate::support::shutdown_past_cancel(host);
}

/// Asks for the install under `now` while a turn on `FAKE` that never answers its cancel runs
/// (on a matter of its own), so the install's hold on new sends lasts until it is taken back.
fn install_now(dir: &Dir, host: &Running) {
    let (_, running, _) = matter_with_thread(dir, host, "running", &[], FAKE);
    host.prompt(running, "Draft clause 4.");
    host.wait_armed(DURABLE);
    host.ok(HostCommand::CheckForUpdate {});
    until_ready(host);
    host.ok(HostCommand::InstallUpdate {
        when: InstallWhen::Now,
    });
    host.settle();
}

#[test]
fn prompts_under_an_install_alone_show_update_and_start_nothing_until_it_is_taken_back() {
    let dir = Dir::new(
        "update_holds_install_alone",
        &[
            (FAKE, Scenario::Bundled("cancel-unanswered")),
            (HELD, Scenario::Local("update-two-prompts")),
        ],
    );
    let fake = FakeUpdater::offering(&dir);
    let host = start_updating(&dir, 0, &fake);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "held", &[], HELD);
    let mut events = host.events(StreamId::Thread(thread));
    install_now(&dir, &host);
    assert_eq!(hold_reason(&host, matter, thread), None, "nothing queued");

    // Queued under the install's hold alone: the queue is not held for the lawyer, yet its turns
    // wait, and show it.
    let (one, _) = host.prompt(thread, "First question");
    let (two, _) = host.prompt(thread, "Second question");
    host.settle();
    events.drain_ready();
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Update),
        "{}",
        events.dump()
    );
    // Showing it starts nothing.
    assert_eq!((starts(&events, one), starts(&events, two)), (0, 0));

    // Taken back: the queue moves on, and once it has emptied nothing shows.
    host.ok(HostCommand::CancelScheduledUpdate {});
    assert_eq!(events.until_turn_ended(one).stop, StopReason::EndTurn);
    assert_eq!(events.until_turn_ended(two).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    assert_eq!(hold_reason(&host, matter, thread), None);
    events.assert_no_mismatch();
    crate::support::shutdown_past_cancel(host);
}

/// Waits for the turn put back at the queue's head after `ended` failed for the sign-in.
fn until_put_back(events: &mut Events) -> clauseo_protocol::TurnId {
    let head = |event: &DomainEvent| matches!(event, DomainEvent::TurnQueued(queued) if queued.is_at_head());
    events.until("the turn put back", |events| {
        events.position(head).is_some()
    });
    events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnQueued(queued) if queued.is_at_head() => Some(queued.turn()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_queue_held_for_the_sign_in_under_an_install_shows_sign_in_then_update() {
    let dir = Dir::new(
        "update_holds_sign_in_under_install",
        &[
            (FAKE, Scenario::Bundled("cancel-unanswered")),
            (HELD, Scenario::Bundled("auth-required-then-load")),
        ],
    );
    let fake = FakeUpdater::offering(&dir);
    let host = start_updating(&dir, 0, &fake);
    let held_agent = AgentId::new(HELD).unwrap();
    let (matter, thread, _) = matter_with_thread(&dir, &host, "held", &[], HELD);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "First question");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Failed);
    let again = until_put_back(&mut events);
    host.settle();
    events.drain_ready();
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::SignIn),
        "{}",
        events.dump()
    );

    // The install's hold too: the sign-in, more specific, still shows.
    install_now(&dir, &host);
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::SignIn)
    );

    // `restart_agents` lifts the sign-in's hold: the install's alone holds the turn now.
    wait("restart_agents", host.handle.restart_agents(held_agent)).unwrap();
    host.settle();
    events.drain_ready();
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Update)
    );
    assert_eq!(starts(&events, again), 0, "{}", events.dump());

    host.ok(HostCommand::CancelScheduledUpdate {});
    assert_eq!(events.until_turn_ended(again).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    assert_eq!(hold_reason(&host, matter, thread), None);
    events.assert_no_mismatch();
    crate::support::shutdown_past_cancel(host);
}

#[test]
fn both_holds_lifting_the_update_first_holds_until_the_sign_in_is_lifted() {
    both_holds("update_holds_update_first", First::Update, false);
}

#[test]
fn both_holds_lifting_the_sign_in_first_holds_until_the_update_is_lifted() {
    both_holds("update_holds_sign_in_first", First::SignIn, false);
}

#[test]
fn resume_queue_under_stop_and_update_changes_nothing_then_each_lift_order_runs_once() {
    both_holds("update_holds_resume_update_first", First::Update, true);
    both_holds("update_holds_resume_sign_in_first", First::SignIn, true);
}
