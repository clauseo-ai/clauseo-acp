//! A failed prompt's end waiting for the host's probe (`offline/hold.rs`): a Stop during the
//! wait ends the turn cancelled at once and keeps the agent, a shutdown ends it
//! `thread.interrupted` at once; the window going offline
//! or online during the wait decides the verdict as it stands at the end; an unreachable end
//! carries the probe's fixed fields on the prompt path too; and a cloud provider's login (nothing
//! to probe) under a window that says offline is offline, not a protocol error. The probe is a
//! fixed answer or one held behind a gate: no test here touches the network.

use clauseo_protocol::{AgentKind, CommandOutcome, ErrorCode, HostCommand, StopReason, ThreadId};
use serde_json::json;

use super::agents::send_prompt;
use super::failures::until_ended;
use super::offline::{answer, ended, prompting};
use super::offline_quiet::{PROMPT, chunk, probes, start};
use super::offline_target::EnvHook;
use super::offline_truth::{CONNECTION, detail, failing, window_offline};
use super::rig::Rig;
use crate::actor::turn::offline::probe::{PROBE_CAP, ReachProbe};

/// The second prompt, answered at once.
const SECOND: &str = r#"{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}"#;

/// The probe's fields for a miss of the test probe.
const MISSED: &str = "; probe class=refused via=none status=0";

/// Runs until `thread`'s failed prompt waits for the probe.
fn until_held(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("the end held", |actor| {
        let current = actor.threads[&thread].turn.current.as_ref();
        current.is_some_and(|current| current.probe_hold.is_some())
    });
}

/// The pid of `thread`'s agent.
fn agent_pid(rig: &Rig, thread: ThreadId) -> u32 {
    rig.actor.threads[&thread]
        .turn
        .agent
        .as_deref()
        .unwrap()
        .link
        .pid()
}

/// Stop while the end waits for a probe that never answers: the turn ends cancelled with no
/// clock moved (no 5 s fallback, no 8 s cap), its answer kept, and the same agent answers the
/// next prompt.
#[test]
fn a_stop_during_the_wait_ends_cancelled_at_once_and_keeps_the_agent() {
    let (probe, gate) = ReachProbe::gated(false);
    let mut lines = failing(CONNECTION);
    lines.push(SECOND.to_owned());
    let (mut rig, thread, _) = start("truth-wait-stop", Some(AgentKind::Claude), &lines, probe);
    let (_, turn) = prompting(&mut rig);
    until_held(&mut rig, thread);
    let pid = agent_pid(&rig, thread);
    let stopped = rig.dispatch(30, HostCommand::Cancel { thread });
    assert_eq!(stopped.unwrap(), CommandOutcome::Done);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(ended(&log, turn), (Some(StopReason::Cancelled), None));
    assert_eq!(answer(&log, turn), "Clause five says");
    let next = send_prompt(&mut rig, 40, thread);
    until_ended(&mut rig, thread);
    assert_eq!(agent_pid(&rig, thread), pid, "the agent was stopped");
    assert_eq!(
        ended(&rig.log(thread), next),
        (Some(StopReason::EndTurn), None)
    );
    drop(gate);
    rig.finish();
}

/// Shutdown while the end waits for a probe that never answers: the turn ends
/// `thread.interrupted` (not the lawyer's `Cancelled`) with no clock moved.
#[test]
fn a_shutdown_during_the_wait_ends_interrupted_at_once() {
    let (probe, gate) = ReachProbe::gated(false);
    let lines = failing(CONNECTION);
    let (mut rig, thread, _) = start(
        "truth-wait-shutdown",
        Some(AgentKind::Claude),
        &lines,
        probe,
    );
    let (_, turn) = prompting(&mut rig);
    until_held(&mut rig, thread);
    rig.shut_down();
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::ThreadInterrupted))
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    drop(gate);
    rig.finish();
}

/// A prompt that fails while online, the window going offline (or, from offline, online) before
/// the probe misses: the window at the end decides, offline then, unreachable now.
#[test]
fn the_window_changing_during_the_wait_decides_at_the_end() {
    for (name, before, code) in [
        ("truth-wait-goes-offline", true, ErrorCode::AgentOffline),
        ("truth-wait-goes-online", false, ErrorCode::AgentUnreachable),
    ] {
        let (probe, gate) = ReachProbe::gated(false);
        let lines = failing(CONNECTION);
        let (mut rig, thread, count) = start(name, Some(AgentKind::Claude), &lines, probe);
        if !before {
            window_offline(&mut rig, 20);
        }
        let (_, turn) = prompting(&mut rig);
        until_held(&mut rig, thread);
        let changed = rig.dispatch(21, HostCommand::SetNetwork { online: !before });
        assert_eq!(changed.unwrap(), CommandOutcome::Done, "{name}");
        gate.send(()).unwrap();
        until_ended(&mut rig, thread);
        let log = rig.log(thread);
        assert_eq!(
            ended(&log, turn),
            (Some(StopReason::Failed), Some(code)),
            "{name}"
        );
        assert_eq!(probes(&count), 1, "{name}");
        rig.finish();
    }
}

/// An unreachable end on the prompt path carries the probe's fixed fields after the agent's
/// error, "no probe answered" when the cap ran out first; an offline end carries none.
#[test]
fn a_failed_prompt_s_unreachable_end_carries_the_probe_s_fields() {
    let probe = ReachProbe::fixed(false);
    let lines = failing(CONNECTION);
    let (mut rig, thread, _) = start("truth-detail", Some(AgentKind::Claude), &lines, probe);
    let (_, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let detail_of = detail(&rig.log(thread), turn).unwrap();
    assert!(
        detail_of.ends_with(&format!("{CONNECTION}{MISSED}")),
        "{detail_of}"
    );
    rig.finish();

    let (probe, gate) = ReachProbe::gated(false);
    let (mut rig, thread, _) = start("truth-detail-cap", Some(AgentKind::Claude), &lines, probe);
    let (_, turn) = prompting(&mut rig);
    until_held(&mut rig, thread);
    rig.clock.advance(PROBE_CAP);
    until_ended(&mut rig, thread);
    drop(gate);
    let detail_of = detail(&rig.log(thread), turn).unwrap();
    assert!(detail_of.ends_with("; no probe answered"), "{detail_of}");
    rig.finish();

    let probe = ReachProbe::fixed(false);
    let (mut rig, thread, _) = start("truth-detail-off", Some(AgentKind::Claude), &lines, probe);
    window_offline(&mut rig, 20);
    let (_, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    let detail_of = detail(&log, turn).unwrap();
    assert!(!detail_of.contains("probe"), "{detail_of}");
    rig.finish();
}

/// codex-acp's `_meta` for a `connection` failure of severity `error`: the prompt's `end_turn`
/// once its retries ran out, titled `title`.
fn gave_up(title: &str) -> String {
    let failure = json!({"jetbrains": {"air": {"version": 1, "sessionFailure": {
        "id": "turn-1:error", "revision": 1, "category": "connection", "severity": "error",
        "title": title, "actions": ["retry", "new_session"],
    }}}});
    let result = json!({"stopReason": "end_turn", "_meta": failure});
    json!({"respond": {"to": "p1", "result": result}}).to_string()
}

/// Codex's error once its retries ran out.
const REFUSED: &str = "stream disconnected before completion: Connection refused (os error 61)";

/// A prompt that ended on a `connection` failure the agent gave up on, unreachable: its title,
/// then the probe's fixed fields.
#[test]
fn a_lost_connection_s_unreachable_end_carries_the_probe_s_fields() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        gave_up(REFUSED),
    ];
    let probe = ReachProbe::fixed(false);
    let (mut rig, thread, _) = start("truth-lost-detail", Some(AgentKind::Codex), &lines, probe);
    let (_, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentUnreachable))
    );
    assert_eq!(detail(&log, turn).unwrap(), format!("{REFUSED}{MISSED}"));
    rig.finish();
}

/// A cloud provider's login (Bedrock: nothing to probe, `unknown_service`) whose prompt ends on
/// a lost connection while the window says offline: offline, as the verdict reads it, never
/// counted as reached (which would fail it `agent.protocol_error`).
#[test]
fn a_cloud_login_s_lost_connection_while_the_window_says_offline_is_offline() {
    let probe = ReachProbe::fixed(true);
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        gave_up(REFUSED),
    ];
    let (mut rig, thread, count) =
        start("truth-cloud-lost", Some(AgentKind::Claude), &lines, probe);
    let hook = EnvHook::default();
    hook.set(&[("CLAUDE_CODE_USE_BEDROCK", "1".to_owned())]);
    rig.actor.profile = Box::new(hook);
    window_offline(&mut rig, 20);
    let (_, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    assert_eq!(probes(&count), 0);
    rig.finish();
}
