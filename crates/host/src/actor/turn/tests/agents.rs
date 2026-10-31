//! The turn path with a real `fake_agent`: the prompt goes out only once `TurnStarted` is
//! durable, updates the rate cap holds are logged when the agent exits, and an agent that
//! exits while its turn waits for the writer is detached after the turn's `TurnEnded`.

use clauseo_protocol::{
    CommandOutcome, DomainEvent, HostCommand, PromptBlock, ThreadId, ToolCallStatus, TurnId,
};

use super::rig::{Rig, kind};
use crate::actor::input::Input;
use crate::actor::turn::agents::Stage;
use crate::actor::turn::{AgentReply, Phase, TurnInput, end};
use crate::agent::AgentInput;
use crate::rate::RateCap;

/// One tool call, two updates, then the agent exits mid-turn.
const EXIT_AFTER_UPDATES: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"tc-1","title":"Read brief.txt","kind":"read","status":"pending"}}}
{"update":{"update":{"sessionUpdate":"tool_call_update","toolCallId":"tc-1","status":"in_progress"}}}
{"update":{"update":{"sessionUpdate":"tool_call_update","toolCallId":"tc-1","status":"completed"}}}
{"exit":{"code":1}}
"#;

fn send_prompt(rig: &mut Rig, n: u128, thread: ThreadId) -> TurnId {
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Read the brief.".to_owned(),
        }],
        delivery: clauseo_protocol::DeliveryRequest::Queue,
    };
    let accepted = rig.dispatch(n, command).unwrap();
    let CommandOutcome::PromptAccepted { turn, .. } = accepted else {
        panic!("not accepted: {accepted:?}");
    };
    turn
}

/// Handles inputs until `thread` has no turn, no agent and nothing in flight.
fn until_settled(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("the turn's end and the agent's", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none()
            && state.agent.is_none()
            && actor.work.commits == 0
            && actor.work.matter_jobs == 0
    });
}

#[test]
fn prompt_waits_for_turn_started_and_exit_flushes_the_rate_cap() {
    let mut rig = Rig::start("turn-exit-rate", EXIT_AFTER_UPDATES);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);

    // The prompt is not sent while the `TurnStarted` commit is in flight.
    let started = |actor: &crate::actor::Actor, input: &Input| {
        matches!(input, Input::Committed { ticket, .. }
            if actor.pipeline.events(*ticket).iter()
                .any(|e| matches!(e.event, DomainEvent::TurnStarted(_))))
    };
    rig.hold_next("the TurnStarted commit", started);
    let state = &mut rig.actor.threads.get_mut(&thread).unwrap().turn;
    let current = state.current.as_ref().unwrap();
    assert_eq!(current.phase, Phase::Starting);
    assert!(
        current.prompt_call.is_none(),
        "no prompt before TurnStarted is durable"
    );
    // A cap of one update a window: the agent's two updates are held, merged.
    let agent = state.agent.as_mut().unwrap();
    assert_eq!(agent.stage, Stage::Attached);
    agent.rate = RateCap::with_cap(1);

    rig.release_held();
    // The agent's end is handled before the prompt's failed answer, as after a crash.
    let answer = |_: &crate::actor::Actor, input: &Input| {
        matches!(
            input,
            Input::Turn(TurnInput::Agent {
                reply: AgentReply::Prompt { .. },
                ..
            })
        )
    };
    rig.run_holding("the agent's end", answer, |actor| {
        actor.threads[&thread].turn.agent.is_none()
    });
    rig.release_held();
    until_settled(&mut rig, thread);
    let log = rig.log(thread);
    let completed = log.iter().position(|event| {
        matches!(event, DomainEvent::ToolCallUpdated(u)
            if u.patch.status == Some(ToolCallStatus::Completed))
    });
    let ended = log
        .iter()
        .position(|event| matches!(event, DomainEvent::TurnEnded(ended) if ended.turn == turn));
    assert!(
        completed.zip(ended).is_some_and(|(c, e)| c < e),
        "{:?}",
        log.iter().map(kind).collect::<Vec<_>>()
    );
    rig.finish();
}

#[test]
fn an_agent_exit_while_queued_detaches_after_turn_ended() {
    let mut rig = Rig::start("turn-exit-queued", r#"{"exit":{"code":0}}"#);
    let thread = rig.thread();
    let other = rig.thread_by(2);
    let matter = rig.matter();
    rig.actor.matters.get_mut(&matter).unwrap().turns.holder = Some(other);
    let _turn = send_prompt(&mut rig, 10, thread);

    // The agent's end waits until the turn is queued behind the other thread.
    let exit = |_: &crate::actor::Actor, input: &Input| {
        matches!(
            input,
            Input::Agent {
                input: AgentInput::Ended(_),
                ..
            }
        )
    };
    rig.run_holding("the queued turn", exit, |actor| {
        let state = &actor.threads[&thread].turn;
        state
            .current
            .as_ref()
            .is_some_and(|c| c.phase == Phase::Queued)
            && actor.work.commits == 0
    });
    if rig.held.is_empty() {
        rig.hold_next("the agent's end", exit);
    }
    rig.release_held();
    rig.settle_commits();
    let detached = |log: &[DomainEvent]| {
        log.iter()
            .filter(|e| matches!(e, DomainEvent::AgentSessionDetached(_)))
            .count()
    };
    assert_eq!(detached(&rig.log(thread)), 0, "held until the turn ends");

    // The writer passes on; the turn finds no agent and ends, then the detach is logged.
    end::release_writer(&mut rig.actor, matter, other);
    until_settled(&mut rig, thread);
    let log = rig.log(thread);
    let [
        ..,
        DomainEvent::TurnEnded(_),
        DomainEvent::AgentSessionDetached(_),
    ] = log.as_slice()
    else {
        panic!("{:?}", log.iter().map(kind).collect::<Vec<_>>());
    };
    assert_eq!(detached(&log), 1);
    rig.finish();
}
