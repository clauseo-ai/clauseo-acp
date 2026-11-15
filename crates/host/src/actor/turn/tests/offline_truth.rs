//! The truth of an offline end (`offline/hold.rs`): "You're offline" (`agent.offline`) only while
//! the window says offline and the host's probe misses; else `agent.unreachable`, or for a
//! request that timed out while the probe reached the service, `agent.quiet`. A failed prompt
//! with no fresh probe waits for one, at most `PROBE_CAP`. The probe is a fixed answer (or one
//! held behind a gate): no test here touches the network.

use clauseo_protocol::{
    AgentKind, CommandOutcome, DomainEvent, ErrorCode, HostCommand, StopReason, TurnId,
};

use super::failures::until_ended;
use super::offline::{answer, ended, prompting};
use super::offline_quiet::{
    CANCELLED, PROMPT, chunk, finish_turn, hold, probes, prompting_held, release_one, start, wait,
};
use super::rig::Rig;
use crate::actor::turn::offline::probe::{PROBE_CAP, ReachProbe};
use crate::actor::turn::offline::quiet::QUIET_WAIT;

/// Claude Code's line for a refused connection.
const CLAUDE: &str = "API Error: Unable to connect to API (ECONNREFUSED)";

/// Claude's prompt failing on a connection error.
const CONNECTION: &str = "Internal error: API Error: Connection error.";

/// Claude's prompt failing on a slow service.
const TIMED_OUT: &str = "Internal error: API Error: Request timed out.";

/// A partial answer, then the prompt fails with `message`.
fn failing(message: &str) -> Vec<String> {
    let error = serde_json::json!({"respond": {"to": "p1",
        "error": {"code": -32603, "message": message}}});
    vec![
        PROMPT.to_owned(),
        chunk("Clause five says"),
        error.to_string(),
    ]
}

/// Says the window is offline, as command `n`.
fn window_offline(rig: &mut Rig, n: u128) {
    let answer = rig.dispatch(n, HostCommand::SetNetwork { online: false });
    assert_eq!(answer.unwrap(), CommandOutcome::Done);
}

/// The failure's detail of `turn`.
fn detail(log: &[DomainEvent], turn: TurnId) -> Option<String> {
    log.iter().find_map(|event| {
        if let DomainEvent::TurnFailed(failed) = event
            && failed.turn == turn
        {
            failed.detail.clone()
        } else {
            None
        }
    })
}

/// A Claude turn whose prompt fails with `message`, the window offline when `offline`, the
/// probe answering `reached`: its end code, and the probes made.
fn prompt_fails(name: &str, message: &str, offline: bool, reached: bool) -> (ErrorCode, usize) {
    let probe = ReachProbe::fixed(reached);
    let (mut rig, thread, count) = start(name, Some(AgentKind::Claude), &failing(message), probe);
    if offline {
        window_offline(&mut rig, 20);
    }
    let (_, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    let (stop, code) = ended(&log, turn);
    assert_eq!(stop, Some(StopReason::Failed));
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
    (code.unwrap(), probes(&count))
}

#[test]
fn a_prompt_failing_on_a_connection_while_online_waits_for_a_probe_and_ends_unreachable() {
    let got = prompt_fails("truth-online-miss", CONNECTION, false, false);
    assert_eq!(got, (ErrorCode::AgentUnreachable, 1));
}

#[test]
fn a_prompt_failing_on_a_connection_whose_probe_gets_through_ends_unreachable() {
    let got = prompt_fails("truth-online-reached", CONNECTION, false, true);
    assert_eq!(got, (ErrorCode::AgentUnreachable, 1));
}

#[test]
fn a_request_timed_out_while_the_probe_reaches_the_service_is_a_slow_service() {
    let got = prompt_fails("truth-slow", TIMED_OUT, false, true);
    assert_eq!(got, (ErrorCode::AgentQuiet, 1));
}

#[test]
fn a_request_timed_out_whose_probe_misses_ends_unreachable() {
    let got = prompt_fails("truth-timeout-miss", TIMED_OUT, false, false);
    assert_eq!(got, (ErrorCode::AgentUnreachable, 1));
}

#[test]
fn a_prompt_failing_on_a_connection_while_the_window_says_offline_ends_offline() {
    let got = prompt_fails("truth-window-miss", CONNECTION, true, false);
    assert_eq!(got, (ErrorCode::AgentOffline, 1));
}

/// The window's word alone never makes it offline: the probe got through, so the window is
/// wrong or late, and the failure was the route's.
#[test]
fn a_window_offline_whose_probe_gets_through_ends_unreachable() {
    let got = prompt_fails("truth-window-reached", CONNECTION, true, true);
    assert_eq!(got, (ErrorCode::AgentUnreachable, 1));
}

/// A probe that never answers within `PROBE_CAP` counts as a miss: a slow service's timeout
/// then reads as unreachable, not quiet, though the probe would have got through.
#[test]
fn a_probe_that_does_not_answer_within_the_cap_counts_as_a_miss() {
    let (probe, gate) = ReachProbe::gated(true);
    let lines = failing(TIMED_OUT);
    let (mut rig, thread, count) = start("truth-cap", Some(AgentKind::Claude), &lines, probe);
    let (_, turn) = prompting(&mut rig);
    rig.run_until("the end held", |actor| {
        let current = actor.threads[&thread].turn.current.as_ref();
        current.is_some_and(|current| current.probe_hold.is_some())
    });
    rig.clock.advance(PROBE_CAP);
    until_ended(&mut rig, thread);
    drop(gate);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentUnreachable))
    );
    assert_eq!(probes(&count), 1);
    rig.finish();
}

/// Lines of a silent Claude turn, `extra` after its first words.
fn silent(extra: Option<&str>) -> Vec<String> {
    let mut lines = vec![PROMPT.to_owned(), chunk("Clause five says")];
    lines.extend(extra.map(chunk));
    lines.push(CANCELLED.to_owned());
    lines
}

/// A quiet watch whose two probes miss while the window says online ends unreachable, the
/// probe's fixed fields in its detail.
#[test]
fn two_missed_quiet_probes_while_online_end_unreachable() {
    let probe = ReachProbe::fixed(false);
    let lines = silent(None);
    let (mut rig, thread, _) = start("truth-quiet", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    wait(&mut rig, thread, QUIET_WAIT);
    wait(&mut rig, thread, QUIET_WAIT);
    finish_turn(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentUnreachable))
    );
    let detail = detail(&log, turn).unwrap();
    assert!(
        detail.ends_with("; probe class=refused via=none status=0"),
        "{detail}"
    );
    rig.finish();
}

/// The window going offline after the first miss makes the quiet watch the window's: its next
/// miss ends the turn offline.
#[test]
fn a_quiet_watch_whose_window_goes_offline_ends_offline() {
    let probe = ReachProbe::fixed(false);
    let lines = silent(None);
    let (mut rig, thread, _) = start("truth-quiet-window", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    wait(&mut rig, thread, QUIET_WAIT);
    window_offline(&mut rig, 20);
    wait(&mut rig, thread, QUIET_WAIT);
    finish_turn(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    rig.finish();
}

/// Claude's connection line, then silence and a missed probe: unreachable while online, offline
/// while the window says so; either way the line stays out of the answer.
#[test]
fn claude_s_note_then_a_miss_is_unreachable_online_and_offline_offline() {
    for (name, offline, code) in [
        ("truth-note", false, ErrorCode::AgentUnreachable),
        ("truth-note-window", true, ErrorCode::AgentOffline),
    ] {
        let probe = ReachProbe::fixed(false);
        let lines = silent(Some(CLAUDE));
        let (mut rig, thread, _) = start(name, Some(AgentKind::Claude), &lines, probe);
        if offline {
            window_offline(&mut rig, 20);
        }
        let turn = prompting_held(&mut rig, thread);
        hold(&mut rig, 2);
        release_one(&mut rig);
        release_one(&mut rig);
        wait(&mut rig, thread, QUIET_WAIT);
        finish_turn(&mut rig, thread);
        let log = rig.log(thread);
        assert_eq!(
            ended(&log, turn),
            (Some(StopReason::Failed), Some(code)),
            "{name}"
        );
        assert_eq!(answer(&log, turn), "Clause five says", "{name}");
        rig.finish();
    }
}
