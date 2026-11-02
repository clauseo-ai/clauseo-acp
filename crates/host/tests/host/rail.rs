//! Each matter's status in `ListMatters`, from its threads, at the interface: working, a
//! permission, a question, an answer ready, a stop, a pause at the limit, the precedence when
//! threads differ, `OpenMatter` clearing what ended before it, and a restart.

use std::time::Duration;

use clauseo_host::Clock as _;

use clauseo_protocol::{
    CommandOutcome, DomainEvent, HostCommand, MatterId, MatterStatus, StopReason, StreamId,
};

use crate::permissions::{KINDS_ANSWERS, PERM, answer_from, until_first_request};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

/// One display batch.
const DISPLAY: Duration = Duration::from_millis(16);

/// The cancel fallback's wait.
const CANCEL_WAIT: Duration = Duration::from_secs(5);

/// The status `ListMatters` gives `matter`.
fn status(host: &Running, matter: MatterId) -> MatterStatus {
    host.matters()
        .into_iter()
        .find(|summary| summary.matter == matter)
        .unwrap_or_else(|| panic!("{matter} is not listed"))
        .status
}

#[test]
fn working_then_stopped_then_idle_once_opened() {
    let dir = Dir::new(
        "status_working_stopped",
        &[(FAKE, Scenario::Bundled("cancel-unanswered"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    assert_eq!(status(&host, matter), MatterStatus::Idle, "a new thread");
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find the cap.");
    host.wait_armed(DISPLAY);
    assert_eq!(status(&host, matter), MatterStatus::Working);
    host.ok(HostCommand::Cancel { thread });
    host.wait_armed(CANCEL_WAIT);
    host.clock.advance(CANCEL_WAIT);
    events.until_turn_ended(turn);
    host.settle();
    let MatterStatus::Stopped { at } = status(&host, matter) else {
        panic!("not stopped: {:?}", status(&host, matter));
    };
    assert_eq!(at, host.clock.now().wall);
    host.clock.advance(Duration::from_secs(1));
    host.ok(HostCommand::OpenMatter { matter });
    assert_eq!(
        status(&host, matter),
        MatterStatus::Idle,
        "the stop came before the opening"
    );
    host.shutdown();
}

#[test]
fn a_permission_then_an_answer_ready_which_survives_a_restart() {
    let dir = Dir::new(
        "status_permission_ready",
        &[(PERM, Scenario::Bundled("permission-kinds"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4\n")], PERM);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the four clauses.");
    until_first_request(&mut events);
    assert_eq!(status(&host, matter), MatterStatus::NeedsPermission);
    answer_from(&host, &mut events, thread, 0, &KINDS_ANSWERS);
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.settle();
    assert_eq!(status(&host, matter), MatterStatus::Ready);
    host.shutdown();

    // After a restart the last turn's end comes from the thread's log.
    let host = dir.start(1);
    assert_eq!(status(&host, matter), MatterStatus::Ready);
    assert_eq!(
        host.ok(HostCommand::OpenMatter { matter }),
        CommandOutcome::Done
    );
    assert_eq!(status(&host, matter), MatterStatus::Idle);
    host.shutdown();
    let host = dir.start(2);
    assert_eq!(
        status(&host, matter),
        MatterStatus::Idle,
        "opened stays opened"
    );
    host.shutdown();
}

#[test]
fn a_question_is_needs_answer() {
    let dir = Dir::new(
        "status_question",
        &[(FAKE, Scenario::Bundled("elicitation-form"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    host.prompt(thread, "Who is liable?");
    events.until("ElicitationRequested", |e| {
        e.position(|event| matches!(event, DomainEvent::ElicitationRequested(_)))
            .is_some()
    });
    assert_eq!(status(&host, matter), MatterStatus::NeedsAnswer);
    host.shutdown();
}

#[test]
fn a_stop_at_the_limit_is_paused_at_limit() {
    let dir = Dir::new(
        "status_paused",
        &[(FAKE, Scenario::Bundled("limit-rejected"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Draft the clause.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::AtLimit);
    host.settle();
    let thread_status = host.status(matter, thread);
    let clauseo_protocol::ThreadStatus::PausedAtLimit { resets_at } = thread_status else {
        panic!("the thread is {thread_status:?}");
    };
    assert_eq!(
        status(&host, matter),
        MatterStatus::PausedAtLimit { resets_at }
    );
    host.shutdown();
}

#[test]
fn a_permission_in_one_thread_outranks_an_answer_ready_in_another() {
    let dir = Dir::new(
        "status_precedence",
        &[
            (FAKE, Scenario::Bundled("prompt-stream")),
            (PERM, Scenario::Bundled("permission-kinds")),
        ],
    );
    let host = dir.start(0);
    let (matter, ready, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(ready));
    let (turn, _) = host.prompt(ready, "Summarise.");
    events.until_turn_ended(turn);
    host.settle();
    assert_eq!(status(&host, matter), MatterStatus::Ready);
    let asking = host.create_thread(matter, PERM);
    let mut asks = host.events(StreamId::Thread(asking));
    host.prompt(asking, "Edit the four clauses.");
    until_first_request(&mut asks);
    assert_eq!(status(&host, matter), MatterStatus::NeedsPermission);
    host.shutdown();
}
