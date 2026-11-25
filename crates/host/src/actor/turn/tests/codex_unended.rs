//! A Codex helper is a background helper (`turn/helpers/codex.rs`): a turn that ends by itself
//! leaves it open until its own `HelperEnded`. These tests show that no card stays open for good
//! when that end never comes. In each, the helper was spawned (`HelperSpawned`) in an earlier
//! turn that ended by itself, and codex-acp never says it ended. It is still ended `Stopped`:
//! (1) by a Stop in a later turn; (2) by that Stop's agent stop (`idle::stop_for_helpers`), after
//! which the next prompt loads the session; (3) by the agent's exit with no turn live; (4) by the
//! load after a restart of the app, when the log ends with the helper open and no turn.

use clauseo_protocol::{
    AgentKind, DetachReason, DomainEvent, HelperOutcome, HostCommand, StreamId, ThreadId, ViewEntry,
};
use serde_json::{Value, json};

use super::agents::send_prompt;
use super::codex_helpers::{Codex, LEASE, story};
use super::codex_stop::{helper_frames, reloaded};
use super::failures::until_ended;
use super::helpers::{live_turn, one_turn, session};
use super::helpers_ends::helper_entries;
use super::idle::{idle_stops, until_ended as until_agent_ended};
use super::rig::Rig;
use super::tools_live::rig_adjusted;
use crate::actor::Actor;
use crate::actor::turn::agents;

/// The second prompt, its lead step `tc-2`, the Stop's cancel and its answer.
const STOPPED_SECOND: &str = r#"{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"tc-2","title":"Read","kind":"read","status":"in_progress"}}}
{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p2","result":{"stopReason":"cancelled"}}}
"#;

/// Whether the host has logged the second turn's lead step `tc-2`.
fn second_step_logged(actor: &mut Actor, thread: ThreadId) -> bool {
    actor
        .loaded(StreamId::Thread(thread))
        .is_some_and(|loaded| loaded.sent.tool_call(&session(), "tc-2").is_some())
}

/// How many helpers `thread`'s view leaves open.
fn open_helpers(rig: &mut Rig, thread: ThreadId) -> u32 {
    rig.actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .map_or(0, clauseo_reducer::ThreadState::open_helpers)
}

/// A first turn, answered `end_turn`, in which Codex spawns helper Lease on `gpt-6-luna` and never
/// ends it; the turn's end leaves it open.
pub(super) fn first_turn_leaves_a_helper_open(rig: &mut Rig, thread: ThreadId) {
    let turn = live_turn(rig, thread);
    let mut codex = Codex::new(rig, thread, turn);
    let at = codex.at();
    codex.spawned(at, LEASE, None, "Lease");
    codex.model(LEASE, "gpt-6-luna");
    rig.release_held();
    until_ended(rig, thread);
    assert_eq!(
        open_helpers(rig, thread),
        1,
        "{:?}",
        story(&rig.log(thread))
    );
}

/// (1) A Stop in a later turn ends the helper an earlier turn left open, `Stopped`, in the
/// cancel's own write, before the cancelled turn's end.
#[test]
fn a_codex_helper_left_open_by_an_earlier_turn_ends_stopped_on_a_later_stop() {
    let steps = format!("{}{STOPPED_SECOND}", one_turn("end_turn"));
    let mut rig = Rig::start("codex-unended-stop", &steps);
    let thread = rig.thread();
    first_turn_leaves_a_helper_open(&mut rig, thread);
    send_prompt(&mut rig, 11, thread);
    rig.run_until("the second turn's step", |actor| {
        second_step_logged(actor, thread)
    });
    rig.dispatch(12, HostCommand::Cancel { thread }).unwrap();
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step tc-0 -",
            "start Lease model=gpt-6-luna parent=-",
            "turn_ended EndTurn",
            "step tc-2 -",
            "end Lease Stopped -",
            "turn_ended Cancelled",
        ]
    );
    assert_eq!(open_helpers(&mut rig, thread), 0);
    rig.finish();
}

/// The first launch: a prompt whose helper (X1's, by `fake_agent`'s frames) is never ended and
/// which codex-acp answers `end_turn`, then a second prompt that the Stop cancels.
fn helper_then_stopped_turn() -> Vec<Value> {
    let mut steps = vec![json!({"expect":{"method":"session/prompt","as":"p1",
        "params":{"sessionId":"sess-1"}}})];
    steps.extend(helper_frames());
    steps.push(json!({"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}));
    for line in STOPPED_SECOND.lines() {
        steps.push(serde_json::from_str(line).unwrap());
    }
    steps
}

/// (2) The same Stop through a real Codex connection: the helper the first turn left running is
/// a reason to stop the agent, which is idle-stopped once the cancelled turn has closed; the card
/// reads `Stopped`, and the next prompt relaunches the agent, loads the session (whose replay of
/// the helper makes no card) and is answered.
#[test]
fn a_codex_helper_left_open_by_an_earlier_turn_has_its_agent_stopped_by_a_later_stop() {
    let steps = helper_then_stopped_turn();
    let mut rig = rig_adjusted("codex-unended-agent", &steps, &reloaded(), true, |c| c);
    rig.actor.turn_path.kind_for_tests = Some(AgentKind::Codex);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    assert_eq!(open_helpers(&mut rig, thread), 1);
    send_prompt(&mut rig, 11, thread);
    rig.run_until("the second turn's step", |actor| {
        second_step_logged(actor, thread)
    });
    rig.dispatch(12, HostCommand::Cancel { thread }).unwrap();
    // The cancelled turn closes, then the agent is stopped.
    until_agent_ended(&mut rig, thread);
    assert_eq!(idle_stops(&mut rig, thread), 1, "the agent was not stopped");
    send_prompt(&mut rig, 13, thread);
    rig.run_until("the third turn's end", |actor| {
        actor.threads[&thread].turn.current.is_none() && actor.work.commits == 0
    });
    assert_eq!(
        story(&rig.log(thread)),
        [
            "start Read documents model=gpt-6-luna parent=-",
            "step exec-sleep Read documents",
            "turn_ended EndTurn",
            "step tc-2 -",
            "end Read documents Stopped -",
            "turn_ended Cancelled",
            "turn_ended EndTurn",
        ]
    );
    assert_eq!(open_helpers(&mut rig, thread), 0);
    rig.finish();
}

/// (3) The agent exits with no turn live while the helper an earlier turn left open still runs:
/// the detach ends it `Stopped`, in the detach's write.
#[test]
fn a_codex_helper_left_open_by_an_earlier_turn_ends_stopped_when_the_agent_exits() {
    let mut rig = Rig::start("codex-unended-exit", &one_turn("end_turn"));
    let thread = rig.thread();
    first_turn_leaves_a_helper_open(&mut rig, thread);
    let exited = DetachReason::Exited {
        code: Some(1),
        stderr_tail: String::new(),
    };
    agents::log_detach(&mut rig.actor, thread, exited);
    rig.settle_commits();
    let log = rig.log(thread);
    assert_eq!(
        story(&log),
        [
            "step tc-0 -",
            "start Lease model=gpt-6-luna parent=-",
            "turn_ended EndTurn",
            "end Lease Stopped -",
        ]
    );
    assert!(matches!(
        log.last(),
        Some(DomainEvent::AgentSessionDetached(_))
    ));
    assert_eq!(open_helpers(&mut rig, thread), 0);
    rig.finish();
}

/// (4) The app stops with the helper an earlier turn left open, and the log ends with it open and
/// no turn (a crash, so no detach ends it): the next start's load ends it `Stopped`, and the card
/// reads stopped.
#[test]
fn a_codex_helper_left_open_by_an_earlier_turn_ends_stopped_at_the_load_after_a_restart() {
    let mut rig = Rig::start("codex-unended-restart", &one_turn("end_turn"));
    let thread = rig.thread();
    first_turn_leaves_a_helper_open(&mut rig, thread);
    // The crash takes the run's memory of its helpers with it.
    rig.actor.threads.get_mut(&thread).unwrap().turn.helpers = None;
    let mut rig = rig.restart(1);
    // The load the page asks for ends the helper; the page after it shows it ended.
    helper_entries(&mut rig, thread);
    rig.settle_commits();
    let entries = helper_entries(&mut rig, thread);
    let [ViewEntry::Helper(entry)] = entries.as_slice() else {
        panic!("{entries:?}");
    };
    assert_eq!(entry.outcome, Some(HelperOutcome::Stopped));
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step tc-0 -",
            "start Lease model=gpt-6-luna parent=-",
            "turn_ended EndTurn",
            "end Lease Stopped -",
        ]
    );
    rig.finish();
}
