//! Codex losing its connection mid-answer as codex-acp 2.2.2 tells a client that takes notices
//! and the AIR `sessionFailure` (D-050, stage C): each retry is a `connection` failure, Codex's
//! fallback warning is a notice, and once its retries run out the prompt answers `end_turn`
//! with a `connection` failure of severity `error`. A failure arms the offline watch; 30 s of
//! silence and a probe that cannot get through end the turn `Failed { agent.offline }` with the
//! answer so far kept; a probe that gets through lets the answer go on. None of Codex's text
//! reaches the log. The probe is a fixed answer: no test here touches the network.

use clauseo_protocol::{DomainEvent, ErrorCode, StopReason, ThreadId};
use serde_json::{Value, json};

use super::failures::until_ended;
use super::offline::{answer, ended, prompting};
use super::rig::{Rig, kind};
use crate::actor::input::Input;
use crate::actor::turn::TurnInput;
use crate::actor::turn::offline::probe::ReachProbe;
use crate::actor::turn::offline::{OFFLINE_WAIT, Watch};
use crate::agent::AgentInput;

/// Codex's warning that its WebSocket failed, as stage C's transcript showed it (macOS).
const FALLBACK: &str = "Falling back from WebSockets to HTTPS transport. stream disconnected \
                        before completion: Connection refused (os error 61)";

/// Codex's error once its retries ran out.
const REFUSED: &str = "stream disconnected before completion: Connection refused (os error 61)";

const PROMPT: &str =
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#;

const CANCELLED: &str = r#"{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}"#;

/// A script line sending `update`.
fn update(update: &Value) -> String {
    json!({"update": {"update": update}}).to_string()
}

/// An answer chunk of message `m-1`.
fn chunk(text: &str) -> String {
    update(
        &json!({"sessionUpdate": "agent_message_chunk", "messageId": "m-1",
        "content": {"type": "text", "text": text}}),
    )
}

/// codex-acp's `_meta` for a `connection` failure (`recordSessionFailure`).
fn failure(severity: &str, title: &str) -> Value {
    let actions = if severity == "error" {
        json!(["retry", "new_session"])
    } else {
        json!([])
    };
    json!({"jetbrains": {"air": {"version": 1, "sessionFailure": {
        "id": "turn-1:error", "revision": 1, "category": "connection", "severity": severity,
        "title": title, "actions": actions,
    }}}})
}

/// A retry: a `session_info_update` with a `connection` failure of severity `warning`.
fn retry(title: &str) -> String {
    update(&json!({"sessionUpdate": "session_info_update", "_meta": failure("warning", title)}))
}

/// Codex's fallback warning as a notice (`createWarningEvent`).
fn fallback() -> String {
    update(&json!({"sessionUpdate": "notice", "severity": "warning", "title": FALLBACK}))
}

/// The prompt's `end_turn` with a `connection` failure of severity `error` titled `title`.
fn gave_up(title: &str) -> String {
    let result = json!({"stopReason": "end_turn", "_meta": failure("error", title)});
    json!({"respond": {"to": "p1", "result": result}}).to_string()
}

fn script(lines: &[String]) -> String {
    let mut script = lines.join("\n");
    script.push('\n');
    script
}

/// Whether `input` is an event from the agent's connection: a retry, a chunk.
fn agent_event(input: &Input) -> bool {
    matches!(
        input,
        Input::Agent {
            input: AgentInput::Event(_),
            ..
        }
    )
}

/// The note `thread`'s watch holds, if one stands.
fn note(rig: &Rig, thread: ThreadId) -> Option<String> {
    let current = rig.actor.threads[&thread].turn.current.as_ref()?;
    current.offline.as_ref()?.note().map(str::to_owned)
}

/// No event of `log` the window shows holds any of Codex's own text: only the failure's detail
/// may, which stays in the event log (`offline.rs`).
fn assert_nothing_raw(log: &[DomainEvent]) {
    let shown = log
        .iter()
        .filter(|event| !matches!(event, DomainEvent::TurnFailed(_)));
    for event in shown {
        let shown = format!("{event:?}");
        for raw in [
            "Falling back",
            "Reconnecting",
            "Connection refused",
            "os error",
        ] {
            assert!(!shown.contains(raw), "{raw:?} reached the log: {shown}");
        }
    }
}

#[test]
fn codex_s_retries_then_silence_end_the_turn_offline_and_nothing_raw_is_shown() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        retry("Reconnecting... 2/5"),
        fallback(),
        retry("Reconnecting... waiting for network"),
        CANCELLED.to_owned(),
    ];
    let mut rig = Rig::start("codex-offline-quiet", &script(&lines));
    let (thread, turn) = prompting(&mut rig);
    let last = "Reconnecting... waiting for network";
    rig.run_until("the last retry", |actor| {
        let current = actor.threads[&thread].turn.current.as_ref();
        current
            .and_then(|current| current.offline.as_ref())
            .is_some_and(|watch| watch.note() == Some(last))
    });
    assert_eq!(note(&rig, thread).as_deref(), Some(last));
    // The unit tests' probe never gets through.
    rig.clock.advance(OFFLINE_WAIT);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    let kinds = || log.iter().map(kind).collect::<Vec<_>>();
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline)),
        "{:?}",
        kinds()
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    assert!(
        !log.iter()
            .any(|event| matches!(event, DomainEvent::Notice(_)))
    );
    assert_nothing_raw(&log);
    rig.finish();
}

#[test]
fn a_probe_that_gets_through_after_codex_s_retry_waits_and_the_answer_goes_on() {
    let rest = " the notice period is thirty days.";
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        retry("Reconnecting... waiting for network"),
        chunk(rest),
        r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#.to_owned(),
    ];
    let mut rig = Rig::start("codex-offline-probe-ok", &script(&lines));
    rig.actor.turn_path.probe = ReachProbe::fixed(true);
    let (thread, turn) = prompting(&mut rig);
    // Everything the agent sends after the retry waits until the probe has answered.
    let holding = std::cell::Cell::new(false);
    let hold = |_: &crate::actor::Actor, input: &Input| {
        let from_agent = matches!(
            input,
            Input::Agent {
                input: AgentInput::Event(_),
                ..
            } | Input::Turn(TurnInput::Agent { .. })
        );
        from_agent && holding.get()
    };
    rig.run_holding("the retry's watch", hold, |actor| {
        let armed = actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.offline.is_some());
        if armed {
            holding.set(true);
        }
        armed
    });
    let since = rig.actor.threads[&thread].turn.current.as_ref().unwrap();
    let since = since.offline.as_ref().unwrap().since();
    rig.clock.advance(OFFLINE_WAIT);
    rig.run_holding("the probe's answer", hold, |actor| {
        let current = actor.threads[&thread].turn.current.as_ref();
        current
            .and_then(|current| current.offline.as_ref())
            .is_some_and(|watch| !watch.probing() && watch.since() > since)
    });
    let current = rig.actor.threads[&thread].turn.current.as_ref().unwrap();
    assert!(current.live() && current.failure.is_none());
    rig.release_held();
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(ended(&log, turn), (Some(StopReason::EndTurn), None));
    assert_eq!(answer(&log, turn), format!("Clause five says{rest}"));
    assert_nothing_raw(&log);
    rig.finish();
}

#[test]
fn codex_giving_up_on_its_connection_ends_the_turn_offline() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        retry("Reconnecting... 5/5"),
        gave_up(REFUSED),
    ];
    let mut rig = Rig::start("codex-offline-gave-up", &script(&lines));
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
}

/// codex-acp's own failure when Codex's app-server exits names no connection failure, and no
/// retry came before it: not offline, but the turn fails as the same error over JSON-RPC
/// would, so the cut-off answer is never shown as complete.
#[test]
fn codex_s_app_server_lost_is_not_offline_but_fails() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        gave_up("Connection to Codex was lost."),
    ];
    let mut rig = Rig::start("codex-offline-app-server", &script(&lines));
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (
            Some(StopReason::Failed),
            Some(ErrorCode::AgentProtocolError)
        )
    );
    rig.finish();
}

/// A server's answer in the title is never offline, retry or not; the turn still fails.
#[test]
fn codex_giving_up_on_a_server_s_answer_is_not_offline_but_fails() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        retry("Reconnecting... 5/5"),
        gave_up("unexpected status 502 Bad Gateway"),
    ];
    let mut rig = Rig::start("codex-offline-server", &script(&lines));
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (
            Some(StopReason::Failed),
            Some(ErrorCode::AgentProtocolError)
        )
    );
    rig.finish();
}

/// Retries less than 30 s apart never put the probe off: the wait runs from the first one.
#[test]
fn codex_s_retries_10_s_apart_end_the_turn_offline_30_s_after_the_first() {
    let lines = [
        PROMPT.to_owned(),
        retry("Reconnecting... waiting for network"),
        retry("Reconnecting... waiting for network"),
        retry("Reconnecting... waiting for network"),
        retry("Reconnecting... waiting for network"),
        CANCELLED.to_owned(),
    ];
    let mut rig = Rig::start("codex-offline-retries", &script(&lines));
    let (thread, turn) = prompting(&mut rig);
    let count = std::cell::Cell::new(0);
    let hold = |_: &crate::actor::Actor, input: &Input| {
        let event = agent_event(input);
        if event {
            count.set(count.get() + 1);
        }
        event
    };
    rig.run_holding("the four retries", hold, |_| count.get() == 4);
    let mut retries = std::mem::take(&mut rig.held).into_iter();
    // One retry now and one every 10 s: the third comes 20 s after the first.
    for _ in 0..3 {
        rig.actor.on_input(retries.next().unwrap());
        rig.drain();
        rig.clock.advance(OFFLINE_WAIT / 3);
        rig.drain();
    }
    // 30 s after the first: the wait ran out, and the unit tests' probe never gets through.
    until_ended(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    for late in retries {
        rig.actor.on_input(late);
    }
    rig.finish();
}

/// The host's probe got through, so the computer is online: Codex giving up then is the
/// service's failure, never "You're offline".
#[test]
fn codex_giving_up_after_a_probe_that_got_through_is_not_offline() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        retry("Reconnecting... waiting for network"),
        gave_up(REFUSED),
    ];
    let mut rig = Rig::start("codex-offline-outage", &script(&lines));
    rig.actor.turn_path.probe = ReachProbe::fixed(true);
    let (thread, turn) = prompting(&mut rig);
    // The prompt's answer waits until the probe has answered.
    let hold = |_: &crate::actor::Actor, input: &Input| {
        matches!(input, Input::Turn(TurnInput::Agent { .. }))
    };
    rig.run_holding("the retry's watch", hold, |actor| {
        let current = actor.threads[&thread].turn.current.as_ref();
        current.is_some_and(|current| current.offline.is_some())
    });
    rig.clock.advance(OFFLINE_WAIT);
    rig.run_holding("the probe's answer", hold, |actor| {
        let current = actor.threads[&thread].turn.current.as_ref();
        current
            .and_then(|current| current.offline.as_ref())
            .is_some_and(Watch::reached)
    });
    rig.release_held();
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (
            Some(StopReason::Failed),
            Some(ErrorCode::AgentProtocolError)
        )
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
}

/// A turn the model refused as too long never ends offline, even when Codex then gave up on a
/// connection it had retried (decision 0057's `TooLong` wins over the offline watch).
#[test]
fn a_too_long_refusal_wins_over_codex_giving_up_on_its_connection() {
    let too_long = json!({"jetbrains": {"air": {"version": 1, "sessionFailure": {
        "id": "turn-1:too-long", "revision": 1, "category": "service", "severity": "error",
        "title": "Prompt is too long", "actions": ["retry"],
    }}}});
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        retry("Reconnecting... 5/5"),
        update(&json!({"sessionUpdate": "session_info_update", "_meta": too_long})),
        gave_up(REFUSED),
    ];
    let mut rig = Rig::start("codex-offline-too-long", &script(&lines));
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(ended(&log, turn), (Some(StopReason::TooLong), None));
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
}
