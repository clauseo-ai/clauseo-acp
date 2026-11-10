//! The window's network signal (`SetNetwork`, D-050): offline arms the offline watch on every
//! running turn and on each turn that starts while it stands; 30 s with no agent activity, then a
//! host probe that cannot reach the agent's service, ends the turn `Failed { agent.offline }`
//! through the Cancel path; a probe that gets through, or a tool call still running, makes it
//! wait again; online before then takes the watch back; agent activity clears it; each turn has
//! its own wait. The probe is a fixed answer: no test here touches the network.

use clauseo_protocol::{CommandOutcome, ErrorCode, HostCommand, StopReason, ThreadId};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::offline::{answer, ended, prompting};
use super::rig::Rig;
use crate::actor::turn::offline::OFFLINE_WAIT;
use crate::actor::turn::offline::probe::ReachProbe;

/// A prompt the agent never answers until the host cancels it.
const SILENT: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}
"#;

/// A prompt the agent answers a little of at once, then nothing until the host cancels it.
const STARTED: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{"type":"text","text":"Clause five says"}}}}
{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}
"#;

/// Two prompts: the first answered at once, the second never until the host cancels it.
const TWO: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}
{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p2","result":{"stopReason":"cancelled"}}}
"#;

/// A tool call that starts at once and is still running when the host cancels the prompt.
const TOOL: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"t-1","title":"Run the check","kind":"execute","status":"in_progress"}}}
{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}
"#;

/// A rig playing `script` whose host probe answers `reached`.
fn start(name: &str, script: &str, reached: bool) -> Rig {
    let mut rig = Rig::start(name, script);
    rig.actor.turn_path.probe = ReachProbe::fixed(reached);
    rig
}

/// When `thread`'s watch was last armed, if it stands and no probe runs for it.
fn armed(actor: &crate::actor::Actor, thread: ThreadId) -> Option<std::time::Duration> {
    let current = actor.threads[&thread].turn.current.as_ref()?;
    let watch = current.offline.as_ref().filter(|watch| !watch.probing())?;
    Some(watch.since())
}

/// Whether `thread`'s turn is live with no failure.
fn going(rig: &Rig, thread: ThreadId) -> bool {
    let current = rig.actor.threads[&thread].turn.current.as_ref();
    current.is_some_and(|current| current.live() && current.failure.is_none())
}

/// Cancels `thread`'s turn as the lawyer would; it ends `Cancelled`.
fn cancel(rig: &mut Rig, thread: ThreadId, turn: clauseo_protocol::TurnId) {
    rig.dispatch(30, HostCommand::Cancel { thread }).unwrap();
    until_ended(rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Cancelled), None)
    );
}

/// Sends `SetNetwork` as command `n`: answered `Done`.
fn network(rig: &mut Rig, n: u128, online: bool) {
    let answer = rig.dispatch(n, HostCommand::SetNetwork { online });
    assert_eq!(answer.unwrap(), CommandOutcome::Done);
}

/// Whether `thread`'s current turn is watched.
fn watched(rig: &Rig, thread: ThreadId) -> bool {
    rig.actor.threads[&thread]
        .turn
        .current
        .as_ref()
        .is_some_and(|current| current.offline.is_some())
}

/// Lets due timers fire and the work they start run.
fn wait(rig: &mut Rig, how_long: std::time::Duration) {
    rig.clock.advance(how_long);
    rig.run_tasks_until_stalled();
}

#[test]
fn offline_then_30_s_of_silence_and_a_failed_probe_ends_the_running_turn_offline() {
    let mut rig = start("network-silence", SILENT, false);
    let (thread, turn) = prompting(&mut rig);
    network(&mut rig, 20, false);
    assert!(watched(&rig, thread));
    wait(&mut rig, OFFLINE_WAIT);
    until_ended(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    rig.finish();
}

#[test]
fn online_before_the_wait_ends_takes_the_watch_back() {
    let mut rig = start("network-back", SILENT, false);
    let (thread, turn) = prompting(&mut rig);
    network(&mut rig, 20, false);
    wait(&mut rig, OFFLINE_WAIT / 2);
    network(&mut rig, 21, true);
    assert!(!watched(&rig, thread));
    wait(&mut rig, OFFLINE_WAIT);
    let current = rig.actor.threads[&thread].turn.current.as_ref().unwrap();
    assert!(current.live() && current.failure.is_none());
    rig.dispatch(22, HostCommand::Cancel { thread }).unwrap();
    until_ended(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Cancelled), None)
    );
    rig.finish();
}

#[test]
fn a_turn_that_starts_offline_is_watched_and_agent_activity_clears_it() {
    let mut rig = start("network-activity", STARTED, false);
    let thread = rig.thread();
    network(&mut rig, 20, false);
    let turn = send_prompt(&mut rig, 10, thread);
    // The answer's first words clear the watch the turn's start armed.
    rig.run_until("the answer's first words", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| {
                current.live() && current.offline.is_none() && actor.work.commits == 0
            })
    });
    wait(&mut rig, OFFLINE_WAIT * 2);
    assert!(
        rig.actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .unwrap()
            .failure
            .is_none()
    );
    rig.dispatch(22, HostCommand::Cancel { thread }).unwrap();
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(ended(&log, turn), (Some(StopReason::Cancelled), None));
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
}

#[test]
fn each_turn_waits_its_own_30_s() {
    let mut rig = start("network-per-turn", TWO, false);
    let thread = rig.thread();
    network(&mut rig, 20, false);
    let first = send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), first),
        (Some(StopReason::EndTurn), None)
    );
    // The second turn starts 20 s into the first's wait, so the first's timer finds it.
    wait(&mut rig, OFFLINE_WAIT * 2 / 3);
    let second = send_prompt(&mut rig, 11, thread);
    rig.run_until("the second turn's watch", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.turn == second && current.offline.is_some())
    });
    wait(&mut rig, OFFLINE_WAIT / 3);
    assert!(
        watched(&rig, thread),
        "the first turn's timer ended the second"
    );
    wait(&mut rig, OFFLINE_WAIT * 2 / 3);
    until_ended(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), second),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    rig.finish();
}

#[test]
fn a_probe_that_gets_through_keeps_a_window_only_watch_waiting() {
    let mut rig = start("network-probe-ok", SILENT, true);
    let (thread, turn) = prompting(&mut rig);
    network(&mut rig, 20, false);
    let mut since = armed(&rig.actor, thread).unwrap();
    // Two waits run out; each time the probe gets through and the watch is armed again.
    for _ in 0..2 {
        wait(&mut rig, OFFLINE_WAIT);
        rig.run_until("the probe's answer", |actor| {
            armed(actor, thread).is_some_and(|armed| armed > since)
        });
        since = armed(&rig.actor, thread).unwrap();
        assert!(going(&rig, thread));
    }
    cancel(&mut rig, thread, turn);
    rig.finish();
}

#[test]
fn a_running_tool_call_is_never_ended_by_the_window_s_offline() {
    let mut rig = start("network-tool", TOOL, false);
    let (thread, turn) = prompting(&mut rig);
    rig.run_until("the tool call", |actor| {
        actor
            .loaded(clauseo_protocol::StreamId::Thread(thread))
            .is_some_and(|loaded| loaded.sent.unfinished_tools(turn).next().is_some())
    });
    network(&mut rig, 20, false);
    // 60 s of silence while the tool runs: the wait runs out twice and is armed again.
    let since = armed(&rig.actor, thread).unwrap();
    for _ in 0..2 {
        rig.clock.advance(OFFLINE_WAIT);
        rig.drain();
    }
    assert!(going(&rig, thread));
    assert!(armed(&rig.actor, thread).is_some_and(|armed| armed > since));
    cancel(&mut rig, thread, turn);
    rig.finish();
}
