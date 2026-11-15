//! The tool calls a turn completes at its end after the ACP reader dropped their update with no
//! status (`AcpEvent::UpdateSkipped`, `turn/skipped.rs`), with the events put in by hand where
//! the wire cannot place them: past 64 named calls the turn completes every unfinished one; a
//! call named in a later turn is completed at that turn's end; an agent-started turn keeps what
//! names it; a late or replayed event is dropped; a cancelled turn cancels and a failed turn
//! leaves its calls, and neither passes what it kept to the next turn.

use std::collections::BTreeMap;

use clauseo_acp::{AcpEvent, AgentUpdate, Origin};
use clauseo_protocol::{
    AgentSessionId, CompactString, DomainEvent, HostCommand, StopReason, StreamId, ThreadId,
    ToolCallStatus, TurnId,
};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::offline::ended;
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::{AgentReply, TurnInput};
use crate::agent::AgentInput;

fn session() -> AgentSessionId {
    AgentSessionId::new("sess-1").unwrap()
}

/// The script line expecting prompt `label`.
fn prompt(label: &str) -> String {
    format!(
        r#"{{"expect":{{"method":"session/prompt","as":"{label}","params":{{"sessionId":"sess-1"}}}}}}"#
    )
}

/// The script line answering prompt `label` with `stop`.
fn respond(label: &str, stop: &str) -> String {
    format!(r#"{{"respond":{{"to":"{label}","result":{{"stopReason":"{stop}"}}}}}}"#)
}

/// The script line starting tool call `tc-{n}`, running.
fn tool(n: usize) -> String {
    format!(
        r#"{{"update":{{"update":{{"sessionUpdate":"tool_call","toolCallId":"tc-{n}","title":"Read","kind":"read","status":"in_progress"}}}}}}"#
    )
}

/// The script of `lines`.
fn script(lines: impl IntoIterator<Item = String>) -> String {
    let mut script: String = lines.into_iter().map(|line| line + "\n").collect();
    script.shrink_to_fit();
    script
}

/// Prompt 1 starting `tc-0` up to `tc-{tools - 1}`, then ending `stop`.
fn one_turn(tools: usize, stop: &str) -> String {
    let mut lines = vec![prompt("p1")];
    lines.extend((0..tools).map(tool));
    lines.push(respond("p1", stop));
    script(lines)
}

/// `thread`'s agent says the reader dropped an update of `tool` (unknown when `None`).
fn skip(rig: &mut Rig, thread: ThreadId, origin: Origin, tool: Option<&str>) {
    let event = AcpEvent::UpdateSkipped {
        session: Some(session()),
        origin,
        acp_tool_call_id: tool.map(CompactString::from),
    };
    let input = Input::Agent {
        thread,
        input: AgentInput::Event(Box::new(event)),
    };
    rig.actor.on_input(input);
}

fn prompt_answer(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Turn(TurnInput::Agent {
            reply: AgentReply::Prompt { .. },
            ..
        })
    )
}

fn agent_ended(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Agent {
            input: AgentInput::Ended(_),
            ..
        }
    )
}

/// Whether the host has `tc-0` up to `tc-{tools - 1}` of `thread`.
fn tools_known(actor: &mut Actor, thread: ThreadId, tools: usize) -> bool {
    let Some(loaded) = actor.loaded(StreamId::Thread(thread)) else {
        return false;
    };
    (0..tools).all(|n| {
        loaded
            .sent
            .tool_call(&session(), &format!("tc-{n}"))
            .is_some()
    })
}

/// Sends prompt `n` to `thread` and holds its answer back once the turn's first `tools` calls
/// are logged: the turn is live, and ends when the test releases it.
fn answer_held(rig: &mut Rig, n: u128, thread: ThreadId, tools: usize) -> TurnId {
    let turn = send_prompt(rig, n, thread);
    rig.run_holding("the turn's tool calls", prompt_answer, |actor| {
        tools_known(actor, thread, tools)
    });
    if rig.held.is_empty() {
        rig.hold_next("the prompt's answer", prompt_answer);
    }
    turn
}

/// Each tool call's statuses after its start, in the log's order, by its ACP id.
fn statuses(rig: &mut Rig, thread: ThreadId) -> BTreeMap<String, Vec<ToolCallStatus>> {
    let log = rig.log(thread);
    let mut ids = BTreeMap::new();
    let mut statuses = BTreeMap::new();
    for event in &log {
        if let DomainEvent::ToolCallStarted(started) = event {
            ids.insert(started.tool_call, started.acp_id.to_string());
            statuses.insert(started.acp_id.to_string(), Vec::new());
        } else if let DomainEvent::ToolCallUpdated(updated) = event
            && let (Some(acp_id), Some(status)) =
                (ids.get(&updated.tool_call), updated.patch.status)
        {
            statuses.get_mut(acp_id).unwrap().push(status);
        }
    }
    statuses
}

/// `want` for each tool call `tc-{n}` of `0..tools`.
fn each(
    tools: usize,
    want: impl Fn(usize) -> Vec<ToolCallStatus>,
) -> BTreeMap<String, Vec<ToolCallStatus>> {
    (0..tools).map(|n| (format!("tc-{n}"), want(n))).collect()
}

const COMPLETED: ToolCallStatus = ToolCallStatus::Completed;

#[test]
fn a_turn_completes_only_the_calls_named_as_skipped() {
    let mut rig = Rig::start("skipped-named", &one_turn(3, "end_turn"));
    let thread = rig.thread();
    let turn = answer_held(&mut rig, 10, thread, 3);
    skip(&mut rig, thread, Origin::Turn(turn), Some("tc-0"));
    skip(&mut rig, thread, Origin::Turn(turn), Some("tc-0"));
    rig.release_held();
    until_ended(&mut rig, thread);
    let want = each(3, |n| if n == 0 { vec![COMPLETED] } else { Vec::new() });
    assert_eq!(statuses(&mut rig, thread), want);
    rig.finish();
}

#[test]
fn past_64_named_calls_a_turn_completes_every_unfinished_call() {
    let mut rig = Rig::start("skipped-past-cap", &one_turn(66, "end_turn"));
    let thread = rig.thread();
    let turn = answer_held(&mut rig, 10, thread, 66);
    // 64 named: only they are completed. One more: every unfinished call, `tc-65` too.
    for n in 0..65 {
        skip(
            &mut rig,
            thread,
            Origin::Turn(turn),
            Some(&format!("tc-{n}")),
        );
    }
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(statuses(&mut rig, thread), each(66, |_| vec![COMPLETED]));
    rig.finish();
}

#[test]
fn a_call_from_an_earlier_turn_named_as_skipped_is_completed() {
    let steps = script([
        prompt("p1"),
        tool(0),
        respond("p1", "end_turn"),
        prompt("p2"),
        respond("p2", "end_turn"),
    ]);
    let mut rig = Rig::start("skipped-earlier-turn", &steps);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    // The first turn's end leaves the call open: no dropped update named it.
    assert_eq!(statuses(&mut rig, thread), each(1, |_| Vec::new()));
    let second = answer_held(&mut rig, 11, thread, 1);
    skip(&mut rig, thread, Origin::Turn(second), Some("tc-0"));
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(statuses(&mut rig, thread), each(1, |_| vec![COMPLETED]));
    rig.finish();
}

/// Whether `input` is the agent's usage update with a cost, which ends its own turn.
fn usage_with_cost(_: &Actor, input: &Input) -> bool {
    let Input::Agent {
        input: AgentInput::Event(event),
        ..
    } = input
    else {
        return false;
    };
    matches!(
        &**event,
        AcpEvent::Update {
            update: AgentUpdate::Usage { cost: Some(_), .. },
            ..
        }
    )
}

#[test]
fn an_agent_started_turn_completes_the_call_an_idle_skip_names() {
    let usage = r#"{"update":{"update":{"sessionUpdate":"usage_update","used":1,"size":10,"cost":{"amount":0.01,"currency":"USD"}}}}"#;
    let text = r#"{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-own","content":{"type":"text","text":"Reading."}}}}"#;
    let steps = script([
        prompt("p1"),
        respond("p1", "end_turn"),
        tool(0),
        tool(1),
        text.to_owned(),
        usage.to_owned(),
    ]);
    let mut rig = Rig::start("skipped-own-turn", &steps);
    let thread = rig.thread();
    let first = send_prompt(&mut rig, 10, thread);
    rig.run_holding("the agent's own turn", usage_with_cost, |actor| {
        let current = actor.threads[&thread].turn.current.as_ref();
        let own = current.is_some_and(|current| current.live() && current.own.is_some());
        own && tools_known(actor, thread, 2)
    });
    if rig.held.is_empty() {
        rig.hold_next("the agent's usage", usage_with_cost);
    }
    let own = rig.actor.threads[&thread]
        .turn
        .current
        .as_ref()
        .unwrap()
        .turn;
    assert_ne!(own, first);
    skip(&mut rig, thread, Origin::Idle, Some("tc-0"));
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), own),
        (Some(StopReason::EndTurn), None)
    );
    let want = each(2, |n| if n == 0 { vec![COMPLETED] } else { Vec::new() });
    assert_eq!(statuses(&mut rig, thread), want);
    rig.finish();
}

#[test]
fn a_late_or_replayed_skip_is_dropped() {
    let mut rig = Rig::start("skipped-late-replay", &one_turn(2, "end_turn"));
    let thread = rig.thread();
    let turn = answer_held(&mut rig, 10, thread, 2);
    // Either one, kept, would complete every unfinished call (no id).
    skip(&mut rig, thread, Origin::Late(turn), None);
    skip(&mut rig, thread, Origin::Replay, None);
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::EndTurn), None)
    );
    assert_eq!(statuses(&mut rig, thread), each(2, |_| Vec::new()));
    rig.finish();
}

#[test]
fn a_cancelled_turn_cancels_its_skipped_calls_and_passes_nothing_on() {
    let cancel = r#"{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}"#;
    let steps = script([
        prompt("p1"),
        tool(0),
        tool(1),
        cancel.to_owned(),
        respond("p1", "cancelled"),
        prompt("p2"),
        tool(2),
        respond("p2", "end_turn"),
    ]);
    let mut rig = Rig::start("skipped-cancelled", &steps);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    rig.run_until("the turn's tool calls", |actor| {
        tools_known(actor, thread, 2)
    });
    skip(&mut rig, thread, Origin::Turn(turn), Some("tc-0"));
    skip(&mut rig, thread, Origin::Turn(turn), None);
    rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
    until_ended(&mut rig, thread);
    assert_eq!(ended(&rig.log(thread), turn).0, Some(StopReason::Cancelled));
    // The next turn's open call stays open: the cancelled turn's skips went with it.
    send_prompt(&mut rig, 12, thread);
    until_ended(&mut rig, thread);
    let cancelled = vec![ToolCallStatus::Cancelled];
    let want = each(3, |n| if n < 2 { cancelled.clone() } else { Vec::new() });
    assert_eq!(statuses(&mut rig, thread), want);
    rig.finish();
}

#[test]
fn a_failed_turn_completes_none_of_its_skipped_calls() {
    let exit = r#"{"exit":{"code":1}}"#;
    let steps = script([prompt("p1"), tool(0), tool(1), exit.to_owned()]);
    let mut rig = Rig::start("skipped-failed", &steps);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    let hold =
        |actor: &Actor, input: &Input| prompt_answer(actor, input) || agent_ended(actor, input);
    rig.run_holding("the turn's tool calls", hold, |actor| {
        tools_known(actor, thread, 2)
    });
    skip(&mut rig, thread, Origin::Turn(turn), Some("tc-0"));
    skip(&mut rig, thread, Origin::Turn(turn), None);
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(ended(&rig.log(thread), turn).0, Some(StopReason::Failed));
    assert_eq!(statuses(&mut rig, thread), each(2, |_| Vec::new()));
    rig.finish();
}
