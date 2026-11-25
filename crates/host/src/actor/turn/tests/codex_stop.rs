//! A Stop with a Codex helper open (`helpers::on_cancel`, `idle::stop_for_helpers`), with a real
//! `fake_agent` playing codex-acp 2.10.0's frames (X1's helper, F1d-restart's replay): the card
//! ends `Stopped` once, the agent's process is ended once the cancelled turn has closed, and the
//! next prompt starts the agent again, loads the session, whose replay of the helper makes no
//! card, and is answered. A Stop with no helper open leaves the agent running.

use clauseo_protocol::{
    AgentKind, DomainEvent, HostCommand, StopReason, StreamId, ThreadId, ViewEntry,
};
use serde_json::{Value, json};

use super::agents::send_prompt;
use super::codex_helpers::story;
use super::idle::{idle_stops, live, until_ended, until_idle};
use super::rig::Rig;
use super::tools_live::rig_adjusted;

/// X1's helper.
const HELPER: &str = "01a20c29-d483-7382-9b0e-1629bbf4cee4";

/// The helper's launch, its options (its model) and a step, as X1 has them.
pub(super) fn helper_frames() -> Vec<Value> {
    vec![
        json!({"update":{"update":{"sessionUpdate":"subagent_spawned","subagentSessionId":HELPER,
            "name":"Read documents","task":"Delegated task for Read documents",
            "capabilities":{}},"raw":true}}),
        json!({"update":{"update":{"sessionUpdate":"config_option_update","configOptions":[{
            "id":"model","name":"Model","category":"model","type":"select",
            "currentValue":"gpt-6-luna","options":[{"value":"gpt-6-luna","name":"6 Luna"}]}]},
            "session":HELPER}}),
        json!({"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"exec-sleep",
            "kind":"execute","title":"sleep 20","status":"in_progress"},"session":HELPER}}),
    ]
}

/// The first launch: a prompt that the Stop cancels, with the helper's frames when `helper`;
/// after the cancel codex-acp says the helper was cancelled (X3.jsonl:40) and answers.
fn stopped_turn(helper: bool) -> Vec<Value> {
    let mut steps = vec![json!({"expect":{"method":"session/prompt","as":"p1",
        "params":{"sessionId":"sess-1"}}})];
    if helper {
        steps.extend(helper_frames());
    } else {
        steps.push(
            json!({"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"tc-0",
            "title":"Read","kind":"read","status":"in_progress"}}}),
        );
    }
    steps.push(json!({"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}));
    if helper {
        steps.push(
            json!({"update":{"update":{"sessionUpdate":"subagent_state_update",
            "subagentSessionId":HELPER,"state":"cancelled"},"raw":true}}),
        );
    }
    steps.push(json!({"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}));
    steps
}

/// The relaunch: `session/load` of the same session, whose replay brings the helper back as
/// `disconnected` (F1d-restart.jsonl:60-62), then the next prompt answered.
pub(super) fn reloaded() -> Value {
    json!([
        {"expect":{"method":"session/load","as":"load2","params":{"sessionId":"sess-1"}}},
        {"update":{"update":{"sessionUpdate":"subagent_spawned","subagentSessionId":HELPER,
            "name":"Read documents","task":"Delegated task for Read documents",
            "capabilities":{}},"raw":true}},
        {"update":{"update":{"sessionUpdate":"config_option_update","configOptions":[{
            "id":"model","name":"Model","category":"model","type":"select",
            "currentValue":"gpt-6-luna","options":[{"value":"gpt-6-luna","name":"6 Luna"}]}]},
            "session":HELPER}},
        {"update":{"update":{"sessionUpdate":"subagent_state_update",
            "subagentSessionId":HELPER,"state":"disconnected"},"raw":true}},
        {"respond":{"to":"load2","result":{}}},
        {"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}},
        {"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-2",
            "content":{"type":"text","text":"You asked for one sub-agent."}}}},
        {"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}
    ])
}

/// A Codex agent that takes `session/load`, playing [`stopped_turn`], then [`reloaded`].
fn codex_rig(test: &str, helper: bool) -> (Rig, ThreadId) {
    let mut rig = rig_adjusted(test, &stopped_turn(helper), &reloaded(), true, |config| {
        config
    });
    rig.actor.turn_path.kind_for_tests = Some(AgentKind::Codex);
    let thread = rig.thread();
    (rig, thread)
}

/// Sends the first prompt and presses Stop once its step is in the view.
fn stop_at_the_step(rig: &mut Rig, thread: ThreadId) {
    send_prompt(rig, 10, thread);
    rig.run_until("the turn's step", |actor| {
        actor
            .loaded(StreamId::Thread(thread))
            .and_then(|loaded| loaded.state.as_thread())
            .is_some_and(|state| {
                let entries = state.entries();
                entries
                    .values()
                    .any(|entry| matches!(entry, ViewEntry::ToolCall(_)))
            })
    });
    rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
}

/// How many turns of `thread` ended with `stop`.
fn ended_with(log: &[DomainEvent], stop: StopReason) -> usize {
    log.iter()
        .filter(|event| matches!(event, DomainEvent::TurnEnded(ended) if ended.stop == stop))
        .count()
}

/// Stop with a Codex helper open: its card ends `Stopped` once (codex-acp's own `cancelled`
/// adds nothing), the agent's process is ended with no prompt waiting, and the next prompt
/// relaunches it, loads the session (the replayed helper makes no second card) and is answered.
#[test]
fn a_stop_with_a_codex_helper_open_ends_the_agent_and_the_next_prompt_reloads_it() {
    let (mut rig, thread) = codex_rig("codex-stop-helper", true);
    stop_at_the_step(&mut rig, thread);
    until_ended(&mut rig, thread);
    assert_eq!(
        idle_stops(&mut rig, thread),
        1,
        "the agent's process was not ended"
    );
    let turn = send_prompt(&mut rig, 12, thread);
    rig.run_until("the second turn's end", |actor| {
        actor.threads[&thread].turn.current.is_none() && actor.work.commits == 0
    });
    let log = rig.log(thread);
    assert_eq!(
        story(&log),
        [
            "start Read documents model=gpt-6-luna parent=-",
            "step exec-sleep Read documents",
            "end Read documents Stopped -",
            "turn_ended Cancelled",
            "turn_ended EndTurn",
        ]
    );
    let answered = log.iter().any(|event| {
        matches!(event, DomainEvent::TurnEnded(ended)
            if ended.turn == turn && ended.stop == StopReason::EndTurn)
    });
    assert!(answered, "{:?}", story(&log));
    assert_eq!(idle_stops(&mut rig, thread), 1);
    rig.finish();
}

/// Stop with no helper open behaves as before: the agent stays running after the cancelled
/// turn, and no process is ended.
#[test]
fn a_stop_with_no_codex_helper_leaves_the_agent_running() {
    let (mut rig, thread) = codex_rig("codex-stop-no-helper", false);
    stop_at_the_step(&mut rig, thread);
    rig.run_until("the cancelled turn's end", |actor| {
        actor.threads[&thread].turn.current.is_none() && actor.work.commits == 0
    });
    until_idle(&mut rig, thread);
    assert!(live(&rig.actor, thread), "the agent was stopped");
    assert_eq!(idle_stops(&mut rig, thread), 0);
    assert_eq!(ended_with(&rig.log(thread), StopReason::Cancelled), 1);
    rig.finish();
}
