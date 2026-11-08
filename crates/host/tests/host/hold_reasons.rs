//! Why a held queue waits, as its header and `ListThreads` row show it (`queue_held_reason`),
//! when a hold starts or ends over a queue the lawyer's cancel already held: an install's hold and
//! `stop_agents`' hold each show over `paused` while they last, and `paused` shows again once
//! they are lifted, each change published to the open view. The other reasons are checked where
//! their holds are (`queue.rs`, `sign_in_hold.rs`, `hold_appends.rs`, `update_holds.rs`).

use clauseo_protocol::{
    AgentId, CommandOutcome, HostCommand, InstallWhen, MatterId, QueueHoldReason, StopReason,
    StreamId, ThreadId, TurnId,
};

use crate::observe::Events;
use crate::queue::{DURABLE, hold_reason, settle, starts, until_queued};
use crate::support::{Dir, FAKE, Running, matter_with_thread, wait};

/// The agent of the thread whose queue is held; its turn is cancelled.
const HELD: &str = "fake-held";

/// A thread on `HELD` whose first turn the lawyer cancelled while a second prompt waited: the
/// queue is held, `paused`. Gives the matter, the thread, its events and the waiting turn.
fn paused(dir: &Dir, host: &Running) -> (MatterId, ThreadId, Events, TurnId) {
    let (matter, thread, _) = matter_with_thread(dir, host, "held", &[("brief.txt", "x\n")], HELD);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Find the cap.");
    host.wait_armed(DURABLE);
    let (second, _) = host.prompt(thread, "Use the 2019 version instead");
    until_queued(&mut events, second);
    assert_eq!(hold_reason(host, matter, thread), None, "before any stop");
    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    settle(host, &mut events);
    assert_eq!(
        hold_reason(host, matter, thread),
        Some(QueueHoldReason::Paused)
    );
    (matter, thread, events, second)
}

#[test]
fn an_install_shows_over_a_paused_queue_and_paused_shows_again_once_it_is_taken_back() {
    let dir = Dir::new(
        "hold_reasons_install",
        &[
            (FAKE, crate::support::Scenario::Bundled("cancel-unanswered")),
            (HELD, crate::support::Scenario::Bundled("send-now")),
        ],
    );
    let fake = crate::update::FakeUpdater::offering(&dir);
    let host = crate::update::start_updating(&dir, 0, &fake);
    let (matter, thread, mut events, second) = paused(&dir, &host);
    // A view kept open keeps the stream loaded, so each fresh view `hold_reason` opens shows the
    // header as the host last published it, which must agree with the row.
    let _open = host.view(StreamId::Thread(thread), None);

    // A turn that never answers its cancel keeps the install's hold up.
    let (_, running, _) = matter_with_thread(&dir, &host, "running", &[], FAKE);
    host.prompt(running, "Draft clause 4.");
    host.wait_armed(DURABLE);
    host.ok(HostCommand::CheckForUpdate {});
    crate::update::until_ready(&host);
    host.ok(HostCommand::InstallUpdate {
        when: InstallWhen::Now,
    });
    settle(&host, &mut events);
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Update)
    );

    host.ok(HostCommand::CancelScheduledUpdate {});
    settle(&host, &mut events);
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Paused),
        "the lawyer's hold is left"
    );
    assert_eq!(starts(&events, second), 0, "{}", events.dump());

    // The lawyer resumes it: the queue moves on and empties.
    host.ok(HostCommand::ResumeQueue { thread });
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(hold_reason(&host, matter, thread), None);
    crate::support::shutdown_past_cancel(host);
}

#[test]
fn stop_agents_shows_switching_over_a_paused_queue_and_paused_shows_again_after_restart() {
    let dir = Dir::new(
        "hold_reasons_stop",
        &[(HELD, crate::support::Scenario::Bundled("send-now"))],
    );
    let host = dir.start(0);
    let (matter, thread, mut events, second) = paused(&dir, &host);
    // A view kept open keeps the stream loaded, so each fresh view `hold_reason` opens shows the
    // header as the host last published it, which must agree with the row.
    let _open = host.view(StreamId::Thread(thread), None);
    let agent = AgentId::new(HELD).unwrap();

    wait("stop_agents", host.handle.stop_agents(agent.clone())).unwrap();
    settle(&host, &mut events);
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Switching)
    );

    // No turn was held for the sign-in, so `restart_agents` resumes nothing: the lawyer's hold
    // shows again.
    wait("restart_agents", host.handle.restart_agents(agent)).unwrap();
    settle(&host, &mut events);
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Paused)
    );
    assert_eq!(starts(&events, second), 0, "{}", events.dump());
    host.shutdown();
}
