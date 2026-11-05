//! An install's hold and the sign-in's hold on one thread at once (decision records 0037 and
//! 0048): a prompt sent while both apply queues held, and stays held until the last of them is
//! lifted, whichever goes first; then each queued prompt runs once, in order.

use clauseo_protocol::{AgentId, DeliveryRequest, HostCommand, InstallWhen, StopReason, StreamId};

use crate::observe::started;
use crate::queue::{DURABLE, held, send, starts};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread, text, wait};
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
    events.assert_no_mismatch();
    host.shutdown_past_cancel();
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
