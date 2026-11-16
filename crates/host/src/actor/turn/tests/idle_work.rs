//! Work an agent does between turns keeps it live (R20b): a case-law relay call in flight
//! counts as pending however long it runs, and every event the agent sends after its turn's end
//! starts its idle clock again, so the stop comes a full limit after the last one.

use std::time::Duration;

use clauseo_acp::{AcpEvent, AgentUpdate, Origin};
use clauseo_protocol::{AgentSessionId, ThreadId};

use super::agents::send_prompt;
use super::idle::{
    MS, TWO_ANSWERS, advance, idle_since, idle_stops, live, pending_of, rig,
    stops_after_a_full_limit, until_ended, until_idle,
};
use super::idle_busy::kept_past_the_limit;
use super::rig::Rig;
use crate::actor::connectors::ConnectorInput;
use crate::actor::input::Input;
use crate::actor::turn::idle::Pending;
use crate::agent::AgentInput;
use crate::config::{IDLE_LIMIT, SessionKey};
use crate::connectors::{RelayCall, RelayEvent};

/// What the relay posts when a message of `session` starts or ends.
fn relay(session: SessionKey, call: RelayCall) -> Input {
    Input::Connector(Box::new(ConnectorInput::Relay(RelayEvent::Call {
        session,
        call,
    })))
}

/// `thread`'s agent sends an update between turns (its plan), handled at once.
fn update_between_turns(rig: &mut Rig, thread: ThreadId) {
    let event = AcpEvent::Update {
        session: AgentSessionId::new("sess-1").unwrap(),
        origin: Origin::Idle,
        update: AgentUpdate::Plan(Vec::new()),
    };
    let input = Input::Agent {
        thread,
        input: AgentInput::Event(Box::new(event)),
    };
    rig.actor.on_input(input);
}

#[test]
fn a_relay_call_open_past_the_limit_keeps_its_agent() {
    let mut rig = rig("idle-relay-call", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    let since = until_idle(&mut rig, thread);
    let key = rig.actor.threads[&thread].turn.run.as_ref().unwrap().key;

    // A message of another launch's relay (a stopped session's) counts for nothing.
    let stale = SessionKey {
        launch: key.launch + 1000,
        ..key
    };
    rig.actor.on_input(relay(stale, RelayCall::Started));
    assert_eq!(pending_of(&rig.actor, thread), None);
    assert_eq!(idle_since(&rig.actor, thread), Some(since));

    // The agent calls the relay between turns: the call holds it past the limit.
    rig.actor.on_input(relay(key, RelayCall::Started));
    kept_past_the_limit(&mut rig, thread, Pending::Call);
    rig.actor.on_input(relay(key, RelayCall::Ended));
    stops_after_a_full_limit(&mut rig, thread);
    rig.finish();
}

#[test]
fn updates_after_the_turn_s_end_push_the_stop_to_a_limit_after_the_last() {
    let mut rig = rig("idle-updates-after", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    let first = until_idle(&mut rig, thread);
    let step = Duration::from_secs(100);

    // Two updates, 100 s apart, after the turn ended: each starts the clock again.
    advance(&mut rig, step);
    update_between_turns(&mut rig, thread);
    assert_eq!(idle_since(&rig.actor, thread), Some(first + step));
    advance(&mut rig, step);
    update_between_turns(&mut rig, thread);
    let last = first + step * 2;
    assert_eq!(idle_since(&rig.actor, thread), Some(last));
    assert_eq!(rig.actor.turn_path.idle.armed_at(), Some(last + IDLE_LIMIT));
    rig.settle_commits();

    // The limit after the turn's end passes: still live.
    advance(&mut rig, IDLE_LIMIT.checked_sub(step * 2).unwrap());
    assert!(
        live(&rig.actor, thread),
        "stopped a limit after the turn's end"
    );
    // A limit after the last update, not a millisecond before.
    advance(&mut rig, (step * 2).checked_sub(MS).unwrap());
    assert!(live(&rig.actor, thread), "stopped before the limit");
    advance(&mut rig, MS);
    until_ended(&mut rig, thread);
    assert_eq!(idle_stops(&mut rig, thread), 1);
    rig.finish();
}
