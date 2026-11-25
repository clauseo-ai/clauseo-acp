//! A switch back whose handover waits for the session's attach (`switch/resume.rs`), stopped
//! while that session loads: the attach comes with no turn to take the handover, so the next
//! message owes it and hands over what the session missed, `resumed`; after a restart too.

use clauseo_protocol::{ContextHandoff, DomainEvent, HandoffStatus, HostCommand, ThreadId};

use super::rig::Rig;
use super::switch::{OTHER, one_turn, send, until_idle};
use crate::actor::turn::agents::Stage;
use crate::actor::turn::{TurnPhase, switch};
use crate::pipeline::tests::support::TempDir;

/// `fake-agent`: it can load sessions; a first launch starts `sess-1` and answers one prompt
/// with text, a later one loads `sess-1` and answers one prompt.
const LOADS: &str = r#"{"expect":{"method":"initialize","as":"init","params":{"protocolVersion":1}}}
{"respond":{"to":"init","result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true},"authMethods":[],"agentInfo":{"name":"fake_agent","version":"0.0.0"}}}}
{"one_of":[[{"expect":{"method":"session/new","as":"new","params":{"cwd":"{{any}}"}}},{"respond":{"to":"new","result":{"sessionId":"sess-1"}}},{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}},{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{"type":"text","text":"Clause 4 is fine."}}}},{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}],[{"expect":{"method":"session/load","as":"load","params":{"sessionId":"sess-1","cwd":"{{any}}"}}},{"respond":{"to":"load","result":{}}},{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}},{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}]]}
"#;

/// A rig whose `fake-agent` plays [`LOADS`] and whose [`OTHER`] answers one prompt with text
/// in `sess-2`.
fn loading(test: &str) -> Rig {
    let dir = TempDir::new(test);
    Rig::prepare(dir.path(), test, "", None);
    let header = |name: &str| {
        format!(
            "{{\"scenario\":\"{name}\",\"description\":\"a unit test\",\
             \"on_stdin_close\":\"exit\"}}\n"
        )
    };
    let scenario = dir.path().join(format!("{test}.jsonl"));
    let text = format!("{}{LOADS}", header(test));
    crate::write::write_file(&scenario, text.as_bytes()).unwrap();
    let path = dir.path().join("data").join("settings.json");
    let mut settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    settings["agents"][OTHER] = settings["agents"]["fake-agent"].clone();
    let other = dir.path().join(format!("{test}-other.jsonl"));
    let handshake =
        super::rig::HANDSHAKE.replace(r#"{"sessionId":"sess-1"}"#, r#"{"sessionId":"sess-2"}"#);
    let steps = super::switch_resume::OTHER_ANSWERS;
    let text = format!("{}{handshake}{steps}", header(&format!("{test}-other")));
    crate::write::write_file(&other, text.as_bytes()).unwrap();
    settings["agents"][OTHER]["args"] =
        serde_json::json!(["--scenario", other.display().to_string()]);
    crate::write::write_file(&path, settings.to_string().as_bytes()).unwrap();
    Rig::open(dir, 0, None)
}

fn handoffs(log: &[DomainEvent]) -> Vec<ContextHandoff> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::ContextHandoff(handoff) = event {
                Some((**handoff).clone())
            } else {
                None
            }
        })
        .collect()
}

/// The status of each `HandoffSettled` naming `handoff`.
fn settled(log: &[DomainEvent], handoff: &ContextHandoff) -> Vec<HandoffStatus> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::HandoffSettled(settled) = event
                && settled.handoff == handoff.handoff
            {
                Some(settled.status)
            } else {
                None
            }
        })
        .collect()
}

/// `fake-agent` answers, the thread moves to [`OTHER`], which answers, then a message moves it
/// back and the lawyer stops it while `fake-agent` loads `sess-1`: the session attaches with
/// no turn running and no handover.
fn stopped_during_the_load(rig: &mut Rig, thread: ThreadId) {
    one_turn(rig, 10, thread);
    rig.dispatch(11, send(thread, Some(OTHER))).unwrap();
    until_idle(rig, thread);
    assert_eq!(handoffs(&rig.log(thread)).len(), 1);
    rig.dispatch(12, send(thread, Some("fake-agent"))).unwrap();
    rig.run_until("the switch back's launch", |actor| {
        let state = &actor.threads[&thread];
        state.row.agent.as_str() == "fake-agent"
            && state
                .turn
                .current
                .as_deref()
                .is_some_and(|current| current.phase == TurnPhase::Launching)
            && state
                .turn
                .agent
                .as_deref()
                .is_some_and(|agent| agent.stage == Stage::Loading)
    });
    rig.dispatch(13, HostCommand::Cancel { thread }).unwrap();
    rig.run_until("the attach, with no turn", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none()
            && state
                .agent
                .as_deref()
                .is_some_and(|agent| agent.stage == Stage::Attached)
    });
    rig.settle_commits();
    assert_eq!(
        handoffs(&rig.log(thread)).len(),
        1,
        "nothing handed over yet"
    );
}

/// The next message hands over what `sess-1` missed: [`OTHER`]'s turn, `resumed`, delivered.
fn hands_over_the_delta(rig: &mut Rig, thread: ThreadId) {
    one_turn(rig, 20, thread);
    let log = rig.log(thread);
    let all = handoffs(&log);
    assert_eq!(all.len(), 2, "{log:?}");
    let owed = &all[1];
    assert!(owed.resumed, "{owed:?}");
    assert_eq!(
        (owed.from.agent.as_str(), owed.to.agent.as_str()),
        (OTHER, "fake-agent")
    );
    // The other agent's answer, the lawyer's message it answered, and the stopped message.
    assert_eq!(
        (owed.answers_covered.get(), owed.items_included.get()),
        (1, 3),
        "{owed:?}"
    );
    assert_eq!(settled(&log, owed), [HandoffStatus::Delivered], "{log:?}");
    assert_eq!(switch::seen(&rig.actor, thread).1, None, "nothing owed now");
}

#[test]
fn a_switch_back_stopped_during_its_load_hands_over_what_was_missed_next_time() {
    let mut rig = loading("resume-stop-load");
    let thread = rig.thread();
    stopped_during_the_load(&mut rig, thread);
    hands_over_the_delta(&mut rig, thread);
    rig.finish();
}

#[test]
fn a_switch_back_stopped_during_its_load_hands_over_what_was_missed_after_a_restart() {
    let mut rig = loading("resume-stop-load-restart");
    let thread = rig.thread();
    stopped_during_the_load(&mut rig, thread);
    let mut rig = rig.restart(1);
    rig.run_until("start-up's repairs", |actor| {
        actor.work.loads == 0 && actor.work.commits == 0
    });
    hands_over_the_delta(&mut rig, thread);
    rig.finish();
}
