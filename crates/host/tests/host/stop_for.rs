//! `stop_agents_for` (decision record 0048): the reason a stop gives is the reason its held
//! queues show (`queue_held_reason`). An account change's stop, `StopFor::SignInAgain`, shows
//! `sign_in`, so the app asks for a sign-in; `stop_agents`' `StopFor::LoginSwitch` still shows
//! `switching`. Either way the turn the stop cancelled is put back held, nothing launches, and
//! `restart_agents` sends it once.

use clauseo_host::StopFor;
use clauseo_protocol::{
    AgentId, DomainEvent, ErrorCode, MatterId, QueueHoldReason, StopReason, StreamId, ThreadId,
    TurnId,
};

use crate::observe::Events;
use crate::queue::{held, hold_reason, settle, starts};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};

/// The prompt every test sends first.
const ASKED: &str = "Look at the matter.";

/// The fake agent's id.
fn fake() -> AgentId {
    AgentId::new(FAKE).unwrap()
}

/// A thread whose first turn is running: the stop cancels it, and the relaunched agent loads
/// the session, so `restart_agents` sends the turn put back.
struct Mid {
    dir: Dir,
    host: Running,
    matter: MatterId,
    thread: ThreadId,
    events: Events,
    first: TurnId,
}

fn mid_turn(test: &str) -> Mid {
    let dir = Dir::new(
        test,
        &[(FAKE, Scenario::Bundled("stop-mid-turn-then-load"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, ASKED);
    events.until("the turn starts", |events| starts(events, first) == 1);
    Mid {
        dir,
        host,
        matter,
        thread,
        events,
        first,
    }
}

/// Waits for the turn put back behind `ended` and gives it.
fn put_back(events: &mut Events, ended: TurnId) -> TurnId {
    let behind = |event: &DomainEvent| matches!(event, DomainEvent::TurnQueued(queued) if queued.behind_turn() == Some(ended));
    events.until("the turn put back", |events| {
        events.position(behind).is_some()
    });
    events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnQueued(queued) if queued.behind_turn() == Some(ended) => {
                Some(queued.turn())
            }
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_sign_in_again_stop_shows_sign_in_and_restart_sends_the_turn_once() {
    let Mid {
        dir: _dir,
        host,
        matter,
        thread,
        mut events,
        first,
    } = mid_turn("stop_for_sign_in");
    wait(
        "stop_agents_for",
        host.handle.stop_agents_for(fake(), StopFor::SignInAgain),
    )
    .unwrap();
    assert_eq!(
        events.until_turn_ended(first).stop,
        StopReason::Cancelled,
        "{}",
        events.dump()
    );
    let again = put_back(&mut events, first);
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "{}", events.dump());
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::SignIn),
        "{}",
        events.dump()
    );
    assert_eq!(starts(&events, again), 0, "{}", events.dump());

    // The question kept: restart_agents sends it once, to the relaunched agent.
    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    assert_eq!(events.until_turn_ended(again).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(hold_reason(&host, matter, thread), None);

    // A second restart resumes nothing.
    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    settle(&host, &mut events);
    events.drain_ready();
    assert_eq!(starts(&events, again), 1, "{}", events.dump());
    let started = events.count(|event| matches!(event, DomainEvent::TurnStarted(_)));
    assert_eq!(started, 2, "{}", events.dump());
    events.assert_no_mismatch();
    host.shutdown();
}

/// R17: desktop's account change refuses the agent's launches with `signin.account_changed`,
/// then stops its agents for a sign-in again. A prompt in another thread of the same agent,
/// sent then, is held for the sign-in (the stop's hold comes before desktop's refusal), never
/// failed; a new sign-in lifts the refusal and restarts the agents, and it runs.
#[test]
fn an_account_change_holds_a_second_threads_prompt_as_sign_in_until_restart() {
    second_thread_held_until_restart(
        "stop_for_account_change",
        StopFor::SignInAgain,
        Some(ErrorCode::SigninAccountChanged),
        QueueHoldReason::SignIn,
    );
}

/// R17's own-to-reused Codex switch: desktop stops the agents for a login switch first (the
/// R20 leave order), then tells the reuse, removes the matter links and restarts. A prompt in
/// another thread of the same agent, sent meanwhile, is held as `switching`, never failed, and
/// runs after the restart.
#[test]
fn a_login_switch_holds_a_second_threads_prompt_as_switching_until_restart() {
    second_thread_held_until_restart(
        "stop_for_own_to_reused",
        StopFor::LoginSwitch,
        None,
        QueueHoldReason::Switching,
    );
}

/// A turn running, then desktop's `refused` refusal (if any) and `stop_agents_for(why)`: a
/// prompt in a second thread of the same matter and agent is held with `reason`, never failed;
/// the refusal lifted and `restart_agents` run both turns.
fn second_thread_held_until_restart(
    test: &str,
    why: StopFor,
    refused: Option<ErrorCode>,
    reason: QueueHoldReason,
) {
    let dir = Dir::new(
        test,
        &[(FAKE, Scenario::Local("account-change-second-thread"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, ASKED);
    events.until("the turn starts", |events| starts(events, first) == 1);

    // Desktop refuses new launches (an account change), then stops the agents.
    if refused.is_some() {
        wait(
            "set_launch_refused",
            host.handle.set_launch_refused(fake(), refused),
        )
        .unwrap();
    }
    wait("stop_agents_for", host.handle.stop_agents_for(fake(), why)).unwrap();
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    let again = put_back(&mut events, first);

    // A prompt in a second thread of the same matter and agent: held, not failed.
    let second = host.create_thread(matter, FAKE);
    let mut second_events = host.events(StreamId::Thread(second));
    let (held_turn, _) = host.prompt(second, "Sent while held.");
    settle(&host, &mut second_events);
    assert!(held(&host, matter, second), "{}", second_events.dump());
    assert_eq!(
        hold_reason(&host, matter, second),
        Some(reason),
        "{}",
        second_events.dump()
    );
    assert_eq!(starts(&second_events, held_turn), 0);
    let failed = second_events.count(|event| matches!(event, DomainEvent::TurnFailed(_)));
    assert_eq!(failed, 0, "{}", second_events.dump());

    // Desktop lifts the refusal and restarts the agents; both turns run.
    if refused.is_some() {
        wait(
            "set_launch_refused",
            host.handle.set_launch_refused(fake(), None),
        )
        .unwrap();
    }
    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    assert_eq!(
        second_events.until_turn_ended(held_turn).stop,
        StopReason::EndTurn,
        "{}",
        second_events.dump()
    );
    assert_eq!(events.until_turn_ended(again).stop, StopReason::EndTurn);
    settle(&host, &mut second_events);
    assert_eq!(hold_reason(&host, matter, second), None);
    second_events.assert_no_mismatch();
    events.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn a_login_switch_stop_still_shows_switching_with_a_turn_put_back() {
    let Mid {
        dir: _dir,
        host,
        matter,
        thread,
        mut events,
        first,
    } = mid_turn("stop_for_switch");
    wait("stop_agents", host.handle.stop_agents(fake())).unwrap();
    assert_eq!(
        events.until_turn_ended(first).stop,
        StopReason::Cancelled,
        "{}",
        events.dump()
    );
    let again = put_back(&mut events, first);
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "{}", events.dump());
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Switching),
        "{}",
        events.dump()
    );
    assert_eq!(starts(&events, again), 0, "{}", events.dump());

    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    assert_eq!(events.until_turn_ended(again).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(hold_reason(&host, matter, thread), None);
    host.shutdown();
}

#[test]
fn the_latest_stop_sets_the_shown_reason() {
    let Mid {
        dir: _dir,
        host,
        matter,
        thread,
        mut events,
        first,
    } = mid_turn("stop_for_latest");
    wait("stop_agents", host.handle.stop_agents(fake())).unwrap();
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    let again = put_back(&mut events, first);
    settle(&host, &mut events);
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Switching)
    );

    // An account change over a login switch: the reason becomes `sign_in`.
    wait(
        "stop_agents_for",
        host.handle.stop_agents_for(fake(), StopFor::SignInAgain),
    )
    .unwrap();
    settle(&host, &mut events);
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::SignIn),
        "{}",
        events.dump()
    );

    // And back: a login switch again shows `switching`, the turn still held.
    wait(
        "stop_agents_for",
        host.handle.stop_agents_for(fake(), StopFor::LoginSwitch),
    )
    .unwrap();
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "{}", events.dump());
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Switching)
    );
    assert_eq!(starts(&events, again), 0, "{}", events.dump());

    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    assert_eq!(events.until_turn_ended(again).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(hold_reason(&host, matter, thread), None);
    host.shutdown();
}
