//! The session's lifetime: `end` revokes the token before it returns (so the token file can go
//! next) and closes the connections, a handshake then gets `closed`, and once the relay is
//! stopped nothing is accepted.

use std::net::TcpStream;

use serde_json::json;

use super::support::*;

#[test]
fn after_end_the_token_no_longer_works_though_its_file_would_still_exist() {
    let rig = rig();
    let mut open = rig.agent();
    rig.session.end();
    // The token file is the caller's to delete after `end`; the token is already dead.
    let mut late = Agent::connect(&rig.session);
    assert_eq!(
        late.hello(rig.token.expose(), "case-law"),
        Some(json!({"clauseo_relay": 1, "ok": false, "error": "closed"}))
    );
    assert!(late.closed());
    // The connection that was open is closed, and nothing it sends is forwarded.
    assert!(open.closed());
    open.send_value(&request(1, "tools/list"));
    assert_eq!(rig.mcp_requests().len(), 0);
}

#[test]
fn closed_is_answered_whatever_the_token_while_the_session_ends() {
    let rig = rig();
    rig.session.end();
    let mut agent = Agent::connect(&rig.session);
    assert_eq!(
        agent.hello(&"1".repeat(64), "case-law"),
        Some(json!({"clauseo_relay": 1, "ok": false, "error": "closed"}))
    );
}

#[test]
fn nothing_is_accepted_after_stop() {
    let rig = rig();
    let mut open = rig.agent();
    let addr = rig.session.addr();
    let Rig { session, .. } = rig;
    session.stop();
    assert!(open.closed());
    assert!(TcpStream::connect(addr).is_err(), "the listener is gone");
}

#[test]
fn turns_move_through_their_handle_after_the_session_is_gone() {
    let rig = rig();
    let turns = rig.session.turns();
    let Rig { session, .. } = rig;
    drop(session);
    turns.start();
    turns.end();
}

#[test]
fn a_listener_off_loopback_is_refused() {
    let rig = rig();
    let wide = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    let err = rig.relays.open(key(3), wide).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    // A loopback one is taken.
    assert!(rig.relays.open(key(4), loopback()).is_ok());
}
