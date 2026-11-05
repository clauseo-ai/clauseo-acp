//! The handshake's refusals: each is answered with its code (or closed unanswered) and closed,
//! and the log names the session and the reason, never the token or the hello.

use futures::executor::block_on;
use serde_json::json;

use super::super::super::{HANDSHAKE_TIMEOUT, MAX_HELLO_BYTES, MAX_PENDING_HANDSHAKES};
use super::support::*;

fn refused(code: &str) -> serde_json::Value {
    json!({"clauseo_relay": 1, "ok": false, "error": code})
}

#[test]
fn a_wrong_token_is_bad_token_and_closed() {
    let rig = rig();
    let mut agent = Agent::connect(&rig.session);
    let wrong = "0".repeat(64);
    assert_eq!(agent.hello(&wrong, "case-law"), Some(refused("bad_token")));
    assert!(agent.closed());
}

#[test]
fn a_token_of_another_listener_is_bad_token() {
    let rig = rig();
    let (other, other_token) = rig.relays.open(key(2), loopback()).unwrap();
    let mut agent = Agent::connect(&rig.session);
    assert_eq!(
        agent.hello(other_token.expose(), "case-law"),
        Some(refused("bad_token"))
    );
    assert!(agent.closed());
    // On its own listener the same token is good, and this listener's is not.
    let mut own = Agent::connect(&other);
    assert_eq!(
        own.hello(other_token.expose(), "case-law"),
        Some(json!({"clauseo_relay": 1, "ok": true}))
    );
    let mut crossed = Agent::connect(&other);
    assert_eq!(
        crossed.hello(rig.token.expose(), "case-law"),
        Some(refused("bad_token"))
    );
}

#[test]
fn a_wrong_connector_is_refused() {
    let rig = rig();
    let mut agent = Agent::connect(&rig.session);
    assert_eq!(
        agent.hello(rig.token.expose(), "other-law"),
        Some(refused("wrong_connector"))
    );
    assert!(agent.closed());
}

#[test]
fn another_version_is_unsupported() {
    let rig = rig();
    let mut agent = Agent::connect(&rig.session);
    let hello = json!({"clauseo_relay": 2, "token": rig.token.expose(), "connector": "case-law"});
    agent.send_value(&hello);
    let answer: serde_json::Value = serde_json::from_slice(&agent.recv_line().unwrap()).unwrap();
    assert_eq!(Some(answer), Some(refused("unsupported_version")));
    assert!(agent.closed());
}

#[test]
fn a_hello_of_the_wrong_shape_closes_unanswered() {
    let rig = rig();
    let token = rig.token.expose();
    let bad = [
        "not json".to_owned(),
        "[1]".to_owned(),
        format!(r#"{{"clauseo_relay":"1","token":"{token}","connector":"case-law"}}"#),
        format!(r#"{{"clauseo_relay":1,"token":"{token}"}}"#),
        format!(r#"{{"clauseo_relay":1,"token":"{token}","connector":"case-law","x":1}}"#),
        format!(
            r#"{{"clauseo_relay":1,"token":"{token}","token":"{token}","connector":"case-law"}}"#
        ),
        format!(
            r#"{{"clauseo_relay":1,"token":"{}","connector":"case-law"}}"#,
            token.to_ascii_uppercase()
        ),
        format!(r#"{{"clauseo_relay":1,"token":"{token}","connector":"-case-law"}}"#),
        format!(r#"{{"clauseo_relay":1,"token":"{token}","connector":"Case-Law"}}"#),
        format!(r#"{{"clauseo_relay":1,"token":"{token}","connector":"case\u002dlaw"}}"#),
        format!(r#"{{"clauseo_relay":1,"token":"{token}","connector":5}}"#),
    ];
    for hello in &bad {
        let mut agent = Agent::connect(&rig.session);
        agent.send(hello);
        assert_eq!(agent.recv_line(), None, "{hello}");
    }
    // A hello over the cap is closed before its newline arrives.
    let mut agent = Agent::connect(&rig.session);
    agent.send_bytes(&vec![b' '; MAX_HELLO_BYTES + 1]);
    assert_eq!(agent.recv_line(), None);
    assert_eq!(rig.mcp_requests().len(), 0);
}

#[test]
fn a_handshake_not_done_in_5_s_is_closed_unanswered() {
    let rig = rig();
    let mut slow = Agent::connect(&rig.session);
    assert_eq!(block_on(rig.clock.next_armed()), HANDSHAKE_TIMEOUT);
    // Most of a hello and no newline: no handshake, and nothing to refuse yet.
    slow.send_raw(br#"{"clauseo_relay":1,"tok"#);
    let mut quick = rig.agent();
    assert_eq!(block_on(rig.clock.next_armed()), HANDSHAKE_TIMEOUT);
    // A round trip on another connection: the relay has had time to act on the slow one.
    quick.send_value(&request(9, "ping"));
    assert_eq!(quick.recv()["id"], 9);
    assert!(
        slow.open_and_quiet(),
        "still waiting on its hello before the deadline"
    );
    rig.clock.advance(HANDSHAKE_TIMEOUT);
    assert_eq!(slow.recv_line(), None);
    // The finished handshake's deadline was disarmed: that connection still works.
    quick.send_value(&request(1, "tools/list"));
    assert_eq!(quick.recv()["id"], 1);
}

#[test]
fn a_fifth_connection_is_busy() {
    let rig = rig();
    let agents = [rig.agent(), rig.agent(), rig.agent(), rig.agent()];
    let mut fifth = Agent::connect(&rig.session);
    assert_eq!(
        fifth.hello(rig.token.expose(), "case-law"),
        Some(refused("busy"))
    );
    assert!(fifth.closed());
    drop(agents);
}

#[test]
fn a_ninth_pending_connection_is_closed_at_once() {
    let rig = rig();
    let pending: Vec<Agent> = (0..MAX_PENDING_HANDSHAKES)
        .map(|_| {
            let agent = Agent::connect(&rig.session);
            // Each admitted connection arms its deadline.
            assert_eq!(block_on(rig.clock.next_armed()), HANDSHAKE_TIMEOUT);
            agent
        })
        .collect();
    let mut ninth = Agent::connect(&rig.session);
    assert!(ninth.closed(), "closed with nothing sent");
    assert_eq!(rig.clock.armed(), MAX_PENDING_HANDSHAKES);
    drop(pending);
}

#[test]
fn a_second_hello_closes_the_connection_unforwarded() {
    let rig = rig();
    let mut agent = rig.agent();
    agent.send(&format!(
        r#"{{"clauseo_relay":1,"token":"{}","connector":"case-law"}}"#,
        rig.token.expose()
    ));
    assert!(agent.closed());
    assert_eq!(rig.mcp_requests().len(), 0);
}

#[test]
fn the_log_names_the_session_and_reason_never_the_token_or_hello() {
    let captured = Captured::default();
    let rig = rig_with(&RigOptions {
        log: Some(captured.dispatch()),
        ..RigOptions::default()
    });
    let token = rig.token.expose().to_owned();
    let mut agent = Agent::connect(&rig.session);
    let wrong = "ab".repeat(32);
    assert_eq!(agent.hello(&wrong, "case-law"), Some(refused("bad_token")));
    let mut shape = Agent::connect(&rig.session);
    let hello =
        format!(r#"{{"clauseo_relay":1,"token":"{token}","connector":"case-law","x":"marker-7"}}"#);
    shape.send(&hello);
    assert_eq!(shape.recv_line(), None);
    let mut good = rig.agent();
    good.send(&format!(
        r#"{{"clauseo_relay":1,"token":"{token}","connector":"case-law"}}"#
    ));
    assert!(good.closed());
    let log = captured.text();
    assert!(log.contains("bad_token"), "{log}");
    assert!(log.contains("bad_hello"), "{log}");
    assert!(log.contains("second_hello"), "{log}");
    assert!(log.contains(&rig.key.matter.to_string()), "{log}");
    assert!(log.contains(&rig.key.id()), "{log}");
    for secret in [token.as_str(), wrong.as_str(), "marker-7", "clauseo_relay"] {
        assert!(!log.contains(secret), "the log holds {secret}: {log}");
    }
}
