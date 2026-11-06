//! The live-agents cap (R20b): on a small machine (here of unknown memory, which counts as
//! small) a launch that finds 2 agents live idle-stops the one idle longest with nothing
//! pending; when both are busy the third agent starts anyway (and the host logs a warning).

use std::time::Duration;

use clauseo_protocol::ThreadId;

use super::agents::send_prompt;
use super::idle::{TWO_ANSWERS, idle_stops, live, start_idle, until_ended, until_idle};
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::agents::Stage;
use crate::actor::turn::{AgentReply, TurnInput};

/// A rig with the cap at 2 and no idle limit, so only the cap stops agents.
fn capped(test: &str) -> Rig {
    start_idle(test, TWO_ANSWERS, "exit", (Duration::MAX, 2))
}

/// Three threads of the Scratch matter.
fn threads(rig: &mut Rig) -> [ThreadId; 3] {
    [rig.thread_by(1), rig.thread_by(2), rig.thread_by(3)]
}

/// Whether `thread`'s agent is attached.
fn attached(actor: &Actor, thread: ThreadId) -> bool {
    actor.threads[&thread]
        .turn
        .agent
        .as_deref()
        .is_some_and(|agent| agent.stage == Stage::Attached)
}

#[test]
fn a_third_agent_idle_stops_the_one_idle_longest() {
    let mut rig = capped("idle-cap-longest");
    let [first, second, third] = threads(&mut rig);
    send_prompt(&mut rig, 10, first);
    until_idle(&mut rig, first);
    rig.clock.advance(Duration::from_secs(10));
    send_prompt(&mut rig, 11, second);
    until_idle(&mut rig, second);
    rig.clock.advance(Duration::from_secs(10));
    // No limit: nothing is armed, however long they idle.
    assert_eq!(rig.actor.turn_path.idle.armed_at(), None);

    send_prompt(&mut rig, 12, third);
    until_ended(&mut rig, first);
    until_idle(&mut rig, third);
    assert_eq!(idle_stops(&mut rig, first), 1);
    assert!(
        live(&rig.actor, second),
        "the agent idle for less was stopped"
    );
    assert_eq!(idle_stops(&mut rig, second), 0);
    assert!(live(&rig.actor, third));
    rig.finish();
}

#[test]
fn a_third_agent_starts_past_the_cap_when_both_are_busy() {
    let mut rig = capped("idle-cap-busy");
    let [first, second, third] = threads(&mut rig);
    // The first turn is held at its prompt's answer; the second waits for the matter's writer
    // behind it: two busy agents.
    send_prompt(&mut rig, 10, first);
    let answer = |_: &Actor, input: &Input| {
        matches!(
            input,
            Input::Turn(TurnInput::Agent {
                reply: AgentReply::Prompt { .. },
                ..
            })
        )
    };
    rig.hold_next("the prompt's answer", answer);
    send_prompt(&mut rig, 11, second);
    rig.run_until("the second agent attached", |actor| attached(actor, second));

    send_prompt(&mut rig, 12, third);
    rig.run_until("the third agent attached", |actor| attached(actor, third));
    for thread in [first, second] {
        let stage = rig.actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .map(|agent| agent.stage);
        assert_eq!(stage, Some(Stage::Attached), "a busy agent was stopped");
    }
    assert!(live(&rig.actor, third));

    rig.release_held();
    until_idle(&mut rig, first);
    until_idle(&mut rig, second);
    until_idle(&mut rig, third);
    for thread in [first, second, third] {
        assert_eq!(idle_stops(&mut rig, thread), 0);
    }
    rig.finish();
}
