//! A switch back (`switch/resume.rs`) whose session's attach commit the store refuses: the
//! handover it carried was never logged, so no `HandoffSettled` names it, and the next turn
//! owes it and carries it.

use clauseo_protocol::{DomainEvent, HandoffId, HandoffStatus};

use super::rig::{HANDSHAKE, Rig};
use super::switch::{OTHER, count, one_turn, send, until_idle};
use crate::actor::turn::switch;
use crate::pipeline::tests::support::TempDir;

/// `fake-agent`'s steps after the handshake: one prompt, answered with text, so its session
/// has seen the conversation.
const ANSWERS: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#,
    "\n",
    r#"{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{"type":"text","text":"Clause 4 is fine."}}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

/// [`OTHER`]'s steps after the handshake: one prompt, answered with text.
const OTHER_ANSWERS: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-2","content":{"type":"text","text":"Clause 5 is not."}}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

/// A rig whose `fake-agent` plays [`ANSWERS`] and whose [`OTHER`] plays [`OTHER_ANSWERS`] in
/// session `sess-2`; each launch reads its scenario from the top.
fn answering(test: &str) -> Rig {
    let dir = TempDir::new(test);
    Rig::prepare(dir.path(), test, ANSWERS, None);
    let path = dir.path().join("data").join("settings.json");
    let mut settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    settings["agents"][OTHER] = settings["agents"]["fake-agent"].clone();
    let scenario = dir.path().join(format!("{test}-other.jsonl"));
    let header = format!(
        "{{\"scenario\":\"{test}-other\",\"description\":\"a unit test\",\
         \"on_stdin_close\":\"exit\"}}\n"
    );
    let handshake = HANDSHAKE.replace(r#"{"sessionId":"sess-1"}"#, r#"{"sessionId":"sess-2"}"#);
    let text = format!("{header}{handshake}{OTHER_ANSWERS}");
    crate::write::write_file(&scenario, text.as_bytes()).unwrap();
    let scenario = scenario.display().to_string();
    settings["agents"][OTHER]["args"] = serde_json::json!(["--scenario", scenario]);
    crate::write::write_file(&path, settings.to_string().as_bytes()).unwrap();
    Rig::open(dir, 0, None)
}

fn handoffs(log: &[DomainEvent]) -> Vec<HandoffId> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::ContextHandoff(handoff) = event {
                Some(handoff.handoff)
            } else {
                None
            }
        })
        .collect()
}

fn settled(log: &[DomainEvent]) -> Vec<(HandoffId, HandoffStatus)> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::HandoffSettled(settled) = event {
                Some((settled.handoff, settled.status))
            } else {
                None
            }
        })
        .collect()
}

/// Back to `fake-agent`, whose session saw the first turn: the attach commit holding the
/// handover is refused. The handover is never settled, since the log does not hold it; the
/// next turn owes it and logs and delivers its own.
#[test]
fn a_refused_attach_never_settles_its_handover_and_the_next_turn_owes_it() {
    let mut rig = answering("resume-attach-refused");
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    let sent = rig.dispatch(11, send(thread, Some(OTHER)));
    assert!(sent.is_ok(), "{sent:?}");
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(handoffs(&log).len(), 1, "{log:?}");

    // The switch back holds its handover for the attach; the store refuses that commit.
    rig.fail(1, |event| matches!(event, DomainEvent::ContextHandoff(_)));
    let sent = rig.dispatch(12, send(thread, Some("fake-agent")));
    assert!(sent.is_ok(), "{sent:?}");
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    let logged = handoffs(&log);
    assert_eq!(
        logged.len(),
        1,
        "the refused handover is not in the log: {log:?}"
    );
    for (handoff, _) in settled(&log) {
        assert!(
            logged.contains(&handoff),
            "a settle names a handover the log does not hold: {log:?}"
        );
    }
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::AgentSwitched(_))),
        2,
        "{log:?}"
    );
    let (_, owed) = switch::seen(&rig.actor, thread);
    assert_eq!(
        owed.as_ref().map(clauseo_protocol::AgentId::as_str),
        Some(OTHER)
    );

    // The next turn carries it: a second handover, logged and delivered.
    one_turn(&mut rig, 13, thread);
    let log = rig.log(thread);
    let logged = handoffs(&log);
    assert_eq!(logged.len(), 2, "{log:?}");
    assert_eq!(
        settled(&log).last(),
        Some(&(logged[1], HandoffStatus::Delivered)),
        "{log:?}"
    );
    for (handoff, _) in settled(&log) {
        assert!(logged.contains(&handoff), "{log:?}");
    }
    assert_eq!(switch::seen(&rig.actor, thread).1, None, "nothing owed now");
    rig.finish();
}
